use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_gateway_protocol as wire;
use psychevo_runtime::{
    Error, ExecutableResolveOptions, HostPlatform, ProcessEnvOptions, apply_tokio_process_env,
    effective_process_env, resolve_executable_path, tokio_host_process_command,
};
use serde_json::Value;

use super::ResolvedScope;
use super::workspace::{path_from_root, resolve_workspace_relative_path};
use file_types::{
    IMAGE_EXTENSIONS, MEDIA_EXTENSIONS, OFFICE_EXTENSIONS, TEXT_EXTENSIONS,
    TEXTUAL_OFFICE_EXTENSIONS, WEBPAGE_EXTENSIONS, is_extension, is_text_filename,
};

mod file_types;

const MAX_TEXT_PROBE_BYTES: u64 = 16 * 1024;
const EXTERNAL_ACTION_ERROR_LIMIT: usize = 240;
const EARLY_EXIT_OBSERVATION: Duration = Duration::from_secs(2);
const EARLY_EXIT_POLL_INTERVAL: Duration = Duration::from_millis(25);

#[derive(Clone)]
pub(super) struct WorkspaceExternalState {
    platform: wire::WorkspaceExternalHostPlatform,
    environment: BTreeMap<String, String>,
    detection_cwd: PathBuf,
    vscode_launcher: Arc<OnceLock<Option<PathBuf>>>,
    launcher: Arc<dyn WorkspaceExternalLauncher>,
}

impl WorkspaceExternalState {
    pub(super) fn production(
        inherited_env: &BTreeMap<String, String>,
        detection_cwd: &Path,
    ) -> Self {
        let platform = current_external_host_platform();
        let environment = effective_process_env(inherited_env, ProcessEnvOptions::new(&[]))
            .unwrap_or_else(|_| inherited_env.clone());
        Self {
            platform,
            environment: environment.clone(),
            detection_cwd: detection_cwd.to_path_buf(),
            vscode_launcher: Arc::new(OnceLock::new()),
            launcher: Arc::new(ProductionWorkspaceExternalLauncher {
                platform,
                environment,
            }),
        }
    }

    fn vscode_launcher(&self) -> Option<&Path> {
        self.vscode_launcher
            .get_or_init(|| {
                detect_vscode_launcher(self.platform, &self.environment, &self.detection_cwd)
            })
            .as_deref()
    }

    #[cfg(test)]
    fn for_test(
        platform: wire::WorkspaceExternalHostPlatform,
        vscode_launcher: Option<PathBuf>,
        launcher: Arc<dyn WorkspaceExternalLauncher>,
    ) -> Self {
        let cache = OnceLock::new();
        cache
            .set(vscode_launcher)
            .expect("test VS Code cache is initialized once");
        Self {
            platform,
            environment: BTreeMap::new(),
            detection_cwd: PathBuf::from("."),
            vscode_launcher: Arc::new(cache),
            launcher,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct WorkspaceExternalLaunchRequest {
    action: wire::WorkspaceExternalFileAction,
    workspace_root: PathBuf,
    file: PathBuf,
    vscode_launcher: Option<PathBuf>,
}

trait WorkspaceExternalLauncher: Send + Sync {
    fn launch(
        &self,
        request: WorkspaceExternalLaunchRequest,
    ) -> BoxFuture<'_, psychevo_runtime::Result<()>>;
}

struct ProductionWorkspaceExternalLauncher {
    platform: wire::WorkspaceExternalHostPlatform,
    environment: BTreeMap<String, String>,
}

impl WorkspaceExternalLauncher for ProductionWorkspaceExternalLauncher {
    fn launch(
        &self,
        request: WorkspaceExternalLaunchRequest,
    ) -> BoxFuture<'_, psychevo_runtime::Result<()>> {
        Box::pin(async move {
            match request.action {
                wire::WorkspaceExternalFileAction::Vscode => {
                    let vscode = request.vscode_launcher.as_deref().ok_or_else(|| {
                        Error::Message("VS Code is not available on the workspace host".to_string())
                    })?;
                    let args = vscode_launch_args(&request.workspace_root, &request.file);
                    spawn_host_command(
                        vscode,
                        &args,
                        &request.workspace_root,
                        host_platform(self.platform),
                        &self.environment,
                        "open the file in VS Code",
                    )
                    .await
                }
                wire::WorkspaceExternalFileAction::SystemDefault => match self.platform {
                    wire::WorkspaceExternalHostPlatform::Macos => {
                        spawn_host_command(
                            Path::new("/usr/bin/open"),
                            &[request.file.clone().into_os_string()],
                            &request.workspace_root,
                            HostPlatform::Posix,
                            &self.environment,
                            "open the file with its default application",
                        )
                        .await
                    }
                    wire::WorkspaceExternalHostPlatform::Linux => {
                        let opener = required_host_executable(
                            "xdg-open",
                            &request.workspace_root,
                            HostPlatform::Posix,
                            &self.environment,
                        )?;
                        spawn_host_command(
                            &opener,
                            &[request.file.clone().into_os_string()],
                            &request.workspace_root,
                            HostPlatform::Posix,
                            &self.environment,
                            "open the file with its default application",
                        )
                        .await
                    }
                    wire::WorkspaceExternalHostPlatform::Windows => {
                        windows_open_default(request.file).await
                    }
                },
                wire::WorkspaceExternalFileAction::Reveal => match self.platform {
                    wire::WorkspaceExternalHostPlatform::Macos => {
                        spawn_host_command(
                            Path::new("/usr/bin/open"),
                            &[OsString::from("-R"), request.file.clone().into_os_string()],
                            &request.workspace_root,
                            HostPlatform::Posix,
                            &self.environment,
                            "show the file in Finder",
                        )
                        .await
                    }
                    wire::WorkspaceExternalHostPlatform::Linux => {
                        reveal_linux_file(&request.file, &request.workspace_root, &self.environment)
                            .await
                    }
                    wire::WorkspaceExternalHostPlatform::Windows => {
                        windows_reveal_file(request.file).await
                    }
                },
            }
        })
    }
}

pub(super) fn workspace_file_external_actions_value(
    state: &WorkspaceExternalState,
    scope: &ResolvedScope,
    path: &str,
) -> psychevo_runtime::Result<Value> {
    let resolved = resolve_regular_workspace_file(scope, path)?;
    let classification = classify_external_file(&resolved)?;
    let vscode_available = classification.text_like && state.vscode_launcher().is_some();
    let preferred_action = if classification.category == wire::WorkspaceExternalFileCategory::Text
        && vscode_available
    {
        wire::WorkspaceExternalFileAction::Vscode
    } else {
        wire::WorkspaceExternalFileAction::SystemDefault
    };
    let available_actions =
        available_actions(preferred_action, classification.text_like, vscode_available);
    Ok(serde_json::to_value(
        wire::WorkspaceFileExternalActionsResult {
            path: workspace_relative_path(&scope.cwd, &resolved)?,
            category: classification.category,
            text_like: classification.text_like,
            platform: state.platform,
            preferred_action,
            available_actions,
        },
    )?)
}

pub(super) async fn workspace_file_open_external_value(
    state: &WorkspaceExternalState,
    scope: &ResolvedScope,
    params: wire::WorkspaceFileOpenExternalParams,
) -> psychevo_runtime::Result<Value> {
    let resolved = resolve_regular_workspace_file(scope, &params.path)?;
    let classification = classify_external_file(&resolved)?;
    let vscode_launcher = if classification.text_like {
        state.vscode_launcher().map(Path::to_path_buf)
    } else {
        None
    };
    let preferred_action = if classification.category == wire::WorkspaceExternalFileCategory::Text
        && vscode_launcher.is_some()
    {
        wire::WorkspaceExternalFileAction::Vscode
    } else {
        wire::WorkspaceExternalFileAction::SystemDefault
    };
    let actions = available_actions(
        preferred_action,
        classification.text_like,
        vscode_launcher.is_some(),
    );
    if !actions.contains(&params.action) {
        return Err(Error::Message(
            "requested external file action is not available".to_string(),
        ));
    }
    let result_path = workspace_relative_path(&scope.cwd, &resolved)?;
    state
        .launcher
        .launch(WorkspaceExternalLaunchRequest {
            action: params.action,
            workspace_root: scope.cwd.clone(),
            file: resolved,
            vscode_launcher,
        })
        .await?;
    Ok(serde_json::to_value(
        wire::WorkspaceFileOpenExternalResult {
            path: result_path,
            action: params.action,
        },
    )?)
}

fn resolve_regular_workspace_file(
    scope: &ResolvedScope,
    path: &str,
) -> psychevo_runtime::Result<PathBuf> {
    let resolved = resolve_workspace_relative_path(&scope.cwd, path)?;
    if !std::fs::metadata(&resolved)?.is_file() {
        return Err(Error::Message(
            "workspace external actions require a regular file".to_string(),
        ));
    }
    Ok(resolved)
}

fn workspace_relative_path(root: &Path, file: &Path) -> psychevo_runtime::Result<String> {
    path_from_root(root, file)
        .ok_or_else(|| Error::Message("workspace path is outside the workspace".to_string()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ExternalFileClassification {
    category: wire::WorkspaceExternalFileCategory,
    text_like: bool,
}

fn classify_external_file(path: &Path) -> psychevo_runtime::Result<ExternalFileClassification> {
    Ok(classify_external_file_with_probe(path, bounded_text_probe))
}

fn classify_external_file_with_probe(
    path: &Path,
    probe: impl FnOnce(&Path) -> psychevo_runtime::Result<bool>,
) -> ExternalFileClassification {
    let extension = path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let filename = path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let (fixed_category, text_like) = if is_extension(&extension, WEBPAGE_EXTENSIONS) {
        (Some(wire::WorkspaceExternalFileCategory::Webpage), true)
    } else if is_extension(&extension, IMAGE_EXTENSIONS) {
        (
            Some(wire::WorkspaceExternalFileCategory::Image),
            extension == "svg",
        )
    } else if is_extension(&extension, MEDIA_EXTENSIONS) {
        (Some(wire::WorkspaceExternalFileCategory::Media), false)
    } else if extension == "pdf" {
        (Some(wire::WorkspaceExternalFileCategory::Pdf), false)
    } else if is_extension(&extension, OFFICE_EXTENSIONS) {
        (
            Some(wire::WorkspaceExternalFileCategory::Office),
            is_extension(&extension, TEXTUAL_OFFICE_EXTENSIONS),
        )
    } else if is_extension(&extension, TEXT_EXTENSIONS) || is_text_filename(&filename) {
        (Some(wire::WorkspaceExternalFileCategory::Text), true)
    } else {
        (None, false)
    };

    if let Some(category) = fixed_category {
        return ExternalFileClassification {
            category,
            text_like,
        };
    }
    let text_like = probe(path).unwrap_or(false);
    ExternalFileClassification {
        category: if text_like {
            wire::WorkspaceExternalFileCategory::Text
        } else {
            wire::WorkspaceExternalFileCategory::Other
        },
        text_like,
    }
}

fn available_actions(
    preferred: wire::WorkspaceExternalFileAction,
    text_like: bool,
    vscode_available: bool,
) -> Vec<wire::WorkspaceExternalFileAction> {
    let mut actions = vec![preferred];
    if preferred == wire::WorkspaceExternalFileAction::Vscode {
        actions.push(wire::WorkspaceExternalFileAction::SystemDefault);
    } else if text_like && vscode_available {
        actions.push(wire::WorkspaceExternalFileAction::Vscode);
    }
    actions.push(wire::WorkspaceExternalFileAction::Reveal);
    actions
}

fn bounded_text_probe(path: &Path) -> psychevo_runtime::Result<bool> {
    let mut bytes = Vec::new();
    Read::by_ref(&mut File::open(path)?)
        .take(MAX_TEXT_PROBE_BYTES)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return Ok(true);
    }
    if bytes.starts_with(&[0xff, 0xfe])
        || bytes.starts_with(&[0xfe, 0xff])
        || bytes.starts_with(&[0xff, 0xfe, 0x00, 0x00])
        || bytes.starts_with(&[0x00, 0x00, 0xfe, 0xff])
    {
        return Ok(true);
    }
    if let Ok(text) = std::str::from_utf8(&bytes) {
        return Ok(text.chars().all(|character| {
            !character.is_control() || matches!(character, '\n' | '\r' | '\t' | '\u{000c}')
        }));
    }
    Ok(bytes.iter().all(|byte| {
        (*byte >= 0x20 && *byte != 0x7f) || matches!(*byte, b'\n' | b'\r' | b'\t' | 0x0c)
    }))
}

fn detect_vscode_launcher(
    platform: wire::WorkspaceExternalHostPlatform,
    environment: &BTreeMap<String, String>,
    cwd: &Path,
) -> Option<PathBuf> {
    let host = host_platform(platform);
    let options = ExecutableResolveOptions {
        platform: host,
        env: environment,
    };
    let well_known = vscode_well_known_paths(platform, environment);
    if platform == wire::WorkspaceExternalHostPlatform::Windows
        && let Some(path) = well_known
            .iter()
            .filter(|candidate| {
                candidate
                    .extension()
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("exe"))
            })
            .find_map(|candidate| {
                resolve_executable_path(&candidate.to_string_lossy(), cwd, &options)
            })
    {
        return Some(path);
    }
    if let Some(path) = resolve_executable_path("code", cwd, &options) {
        return Some(path);
    }
    well_known
        .into_iter()
        .find_map(|candidate| resolve_executable_path(&candidate.to_string_lossy(), cwd, &options))
}

fn vscode_well_known_paths(
    platform: wire::WorkspaceExternalHostPlatform,
    environment: &BTreeMap<String, String>,
) -> Vec<PathBuf> {
    match platform {
        wire::WorkspaceExternalHostPlatform::Macos => {
            let mut paths = vec![PathBuf::from(
                "/Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
            )];
            if let Some(home) = env_value(environment, "HOME") {
                paths.push(
                    PathBuf::from(home).join(
                        "Applications/Visual Studio Code.app/Contents/Resources/app/bin/code",
                    ),
                );
            }
            paths
        }
        wire::WorkspaceExternalHostPlatform::Windows => {
            let mut paths = Vec::new();
            if let Some(local_app_data) = env_value(environment, "LOCALAPPDATA") {
                let install = PathBuf::from(local_app_data).join(r"Programs\Microsoft VS Code");
                paths.push(install.join("Code.exe"));
                paths.push(install.join(r"bin\code.cmd"));
            }
            for variable in ["ProgramFiles", "ProgramFiles(x86)"] {
                if let Some(program_files) = env_value(environment, variable) {
                    let install = PathBuf::from(program_files).join("Microsoft VS Code");
                    paths.push(install.join("Code.exe"));
                    paths.push(install.join(r"bin\code.cmd"));
                }
            }
            paths
        }
        wire::WorkspaceExternalHostPlatform::Linux => vec![
            PathBuf::from("/usr/local/bin/code"),
            PathBuf::from("/snap/bin/code"),
        ],
    }
}

fn env_value<'a>(environment: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    environment
        .iter()
        .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
        .map(|(_, value)| value.as_str())
        .filter(|value| !value.trim().is_empty())
}

fn current_external_host_platform() -> wire::WorkspaceExternalHostPlatform {
    if cfg!(target_os = "macos") {
        wire::WorkspaceExternalHostPlatform::Macos
    } else if cfg!(windows) {
        wire::WorkspaceExternalHostPlatform::Windows
    } else {
        wire::WorkspaceExternalHostPlatform::Linux
    }
}

fn host_platform(platform: wire::WorkspaceExternalHostPlatform) -> HostPlatform {
    match platform {
        wire::WorkspaceExternalHostPlatform::Windows => HostPlatform::Windows,
        wire::WorkspaceExternalHostPlatform::Macos | wire::WorkspaceExternalHostPlatform::Linux => {
            HostPlatform::Posix
        }
    }
}

fn required_host_executable(
    command: &str,
    cwd: &Path,
    platform: HostPlatform,
    environment: &BTreeMap<String, String>,
) -> psychevo_runtime::Result<PathBuf> {
    resolve_executable_path(
        command,
        cwd,
        &ExecutableResolveOptions {
            platform,
            env: environment,
        },
    )
    .ok_or_else(|| Error::Message(format!("{command} is not available on the workspace host")))
}

fn vscode_launch_args(workspace_root: &Path, file: &Path) -> [OsString; 2] {
    [
        workspace_root.as_os_str().to_os_string(),
        file.as_os_str().to_os_string(),
    ]
}

async fn spawn_host_command(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    platform: HostPlatform,
    environment: &BTreeMap<String, String>,
    operation: &str,
) -> psychevo_runtime::Result<()> {
    spawn_host_command_with_observation(
        program,
        args,
        cwd,
        platform,
        environment,
        operation,
        EARLY_EXIT_OBSERVATION,
    )
    .await
}

async fn spawn_host_command_with_observation(
    program: &Path,
    args: &[OsString],
    cwd: &Path,
    platform: HostPlatform,
    environment: &BTreeMap<String, String>,
    operation: &str,
    observation_window: Duration,
) -> psychevo_runtime::Result<()> {
    let mut command = tokio_host_process_command(program, args, platform, environment)?;
    command
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(false);
    apply_tokio_process_env(&mut command, environment, ProcessEnvOptions::new(&[]))?;
    let mut child = command
        .spawn()
        .map_err(|error| bounded_launch_error(operation, &error))?;
    observe_early_process_exit(&mut child, observation_window, operation).await
}

enum EarlyProcessObservation {
    Running,
    Exited(std::process::ExitStatus),
}

async fn observe_early_process_exit(
    child: &mut tokio::process::Child,
    observation_window: Duration,
    operation: &str,
) -> psychevo_runtime::Result<()> {
    let deadline = tokio::time::Instant::now() + observation_window;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return early_process_observation_result(
                    EarlyProcessObservation::Exited(status),
                    operation,
                );
            }
            Ok(None) if tokio::time::Instant::now() >= deadline => {
                return early_process_observation_result(
                    EarlyProcessObservation::Running,
                    operation,
                );
            }
            Ok(None) => {
                let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
                tokio::time::sleep(remaining.min(EARLY_EXIT_POLL_INTERVAL)).await;
            }
            Err(error) => return Err(bounded_launch_error(operation, &error)),
        }
    }
}

fn early_process_observation_result(
    observation: EarlyProcessObservation,
    operation: &str,
) -> psychevo_runtime::Result<()> {
    match observation {
        EarlyProcessObservation::Running => Ok(()),
        EarlyProcessObservation::Exited(status) if status.success() => Ok(()),
        EarlyProcessObservation::Exited(status) => Err(Error::Message(format!(
            "failed to {operation}: opener exited early with {status}"
        ))),
    }
}

async fn reveal_linux_file(
    file: &Path,
    workspace_root: &Path,
    environment: &BTreeMap<String, String>,
) -> psychevo_runtime::Result<()> {
    if let Some(busctl) = resolve_executable_path(
        "busctl",
        workspace_root,
        &ExecutableResolveOptions {
            platform: HostPlatform::Posix,
            env: environment,
        },
    ) {
        let uri = linux_file_uri(file);
        let args = [
            OsString::from("--user"),
            OsString::from("call"),
            OsString::from("org.freedesktop.FileManager1"),
            OsString::from("/org/freedesktop/FileManager1"),
            OsString::from("org.freedesktop.FileManager1"),
            OsString::from("ShowItems"),
            OsString::from("ass"),
            OsString::from("1"),
            OsString::from(uri),
            OsString::new(),
        ];
        let mut command =
            tokio_host_process_command(&busctl, &args, HostPlatform::Posix, environment)?;
        command
            .current_dir(workspace_root)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        apply_tokio_process_env(&mut command, environment, ProcessEnvOptions::new(&[]))?;
        if let Ok(Ok(status)) = tokio::time::timeout(Duration::from_secs(2), command.status()).await
            && status.success()
        {
            return Ok(());
        }
    }

    let parent = file
        .parent()
        .ok_or_else(|| Error::Message("file parent is unavailable".to_string()))?;
    let opener =
        required_host_executable("xdg-open", workspace_root, HostPlatform::Posix, environment)?;
    spawn_host_command(
        &opener,
        &[parent.as_os_str().to_os_string()],
        workspace_root,
        HostPlatform::Posix,
        environment,
        "show the file in its file manager",
    )
    .await
}

#[cfg(unix)]
fn linux_file_uri(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;

    let mut uri = String::from("file://");
    for byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(*byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(char::from(*byte));
        } else {
            use std::fmt::Write as _;
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    uri
}

#[cfg(not(unix))]
fn linux_file_uri(path: &Path) -> String {
    format!("file:///{}", path.to_string_lossy().replace('\\', "/"))
}

#[cfg(windows)]
async fn windows_open_default(path: PathBuf) -> psychevo_runtime::Result<()> {
    tokio::task::spawn_blocking(move || windows_open_default_blocking(&path))
        .await
        .map_err(|error| Error::Message(format!("default application task failed: {error}")))?
}

#[cfg(windows)]
fn windows_open_default_blocking(path: &Path) -> psychevo_runtime::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let _com = initialize_sta_com("default application integration")?;
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            std::ptr::null(),
            path.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    shell_execute_code_result(result as isize)
}

#[cfg(not(windows))]
async fn windows_open_default(_path: PathBuf) -> psychevo_runtime::Result<()> {
    Err(Error::Message(
        "Windows external file opening is unavailable on this host".to_string(),
    ))
}

#[cfg(any(windows, test))]
fn shell_execute_code_result(code: isize) -> psychevo_runtime::Result<()> {
    if code <= 32 {
        Err(Error::Message(format!(
            "failed to open the file with its default application (ShellExecuteW code {code})"
        )))
    } else {
        Ok(())
    }
}

#[cfg(windows)]
async fn windows_reveal_file(path: PathBuf) -> psychevo_runtime::Result<()> {
    tokio::task::spawn_blocking(move || windows_reveal_file_blocking(&path))
        .await
        .map_err(|error| Error::Message(format!("File Explorer task failed: {error}")))?
}

#[cfg(windows)]
fn windows_reveal_file_blocking(path: &Path) -> psychevo_runtime::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::{ILCreateFromPathW, ILFree, SHOpenFolderAndSelectItems};

    let _com = initialize_sta_com("File Explorer integration")?;
    let path = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect::<Vec<_>>();
    let item = unsafe { ILCreateFromPathW(path.as_ptr()) };
    if item.is_null() {
        return Err(Error::Message(
            "failed to resolve the file for File Explorer".to_string(),
        ));
    }
    let result = unsafe { SHOpenFolderAndSelectItems(item, 0, std::ptr::null(), 0) };
    unsafe { ILFree(item) };
    if result < 0 {
        return Err(Error::Message(format!(
            "failed to show the file in File Explorer (HRESULT {result:#x})"
        )));
    }
    Ok(())
}

#[cfg(windows)]
struct StaComGuard;

#[cfg(windows)]
impl Drop for StaComGuard {
    fn drop(&mut self) {
        unsafe { windows_sys::Win32::System::Com::CoUninitialize() };
    }
}

#[cfg(windows)]
fn initialize_sta_com(operation: &str) -> psychevo_runtime::Result<StaComGuard> {
    use windows_sys::Win32::System::Com::{
        COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
    };

    let initialized = unsafe {
        CoInitializeEx(
            std::ptr::null(),
            (COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) as u32,
        )
    };
    if initialized < 0 {
        Err(Error::Message(format!(
            "failed to initialize {operation} (HRESULT {initialized:#x})"
        )))
    } else {
        Ok(StaComGuard)
    }
}

#[cfg(not(windows))]
async fn windows_reveal_file(_path: PathBuf) -> psychevo_runtime::Result<()> {
    Err(Error::Message(
        "Windows file reveal is unavailable on this host".to_string(),
    ))
}

fn bounded_launch_error(operation: &str, error: &std::io::Error) -> Error {
    let mut detail = error.to_string();
    if detail.len() > EXTERNAL_ACTION_ERROR_LIMIT {
        let mut boundary = EXTERNAL_ACTION_ERROR_LIMIT;
        while !detail.is_char_boundary(boundary) {
            boundary -= 1;
        }
        detail.truncate(boundary);
    }
    Error::Message(format!("failed to {operation}: {detail}"))
}

#[cfg(test)]
mod tests;
