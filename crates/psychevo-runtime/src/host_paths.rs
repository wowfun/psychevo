use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::{env, fs};

use crate::error::{Error, Result};

pub const PSYCHEVO_GIT_BASH_PATH: &str = "PSYCHEVO_GIT_BASH_PATH";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathConvention {
    Posix,
    Windows,
    GitBash,
    Cygwin,
    Wsl,
    FileUri,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathRef {
    pub uri: String,
    pub native: String,
    pub display: String,
    pub convention: PathConvention,
}

impl PathRef {
    pub fn native_path(&self) -> PathBuf {
        PathBuf::from(&self.native)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostPlatform {
    Posix,
    Windows,
}

impl HostPlatform {
    pub fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else {
            Self::Posix
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellFamily {
    Posix,
    GitBash,
}

type CygpathResolver<'a> = &'a dyn Fn(&str) -> Result<String>;

#[derive(Clone)]
pub struct PathResolveOptions<'a> {
    pub platform: HostPlatform,
    pub shell_family: ShellFamily,
    pub cygpath: Option<CygpathResolver<'a>>,
}

impl<'a> PathResolveOptions<'a> {
    pub fn current() -> Self {
        Self {
            platform: HostPlatform::current(),
            shell_family: if cfg!(windows) {
                ShellFamily::GitBash
            } else {
                ShellFamily::Posix
            },
            cygpath: None,
        }
    }
}

pub struct ExecutableResolveOptions<'a> {
    pub platform: HostPlatform,
    pub env: &'a BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitBashRuntime {
    pub bash: PathBuf,
    pub cygpath: PathBuf,
}

impl GitBashRuntime {
    pub fn discover(env_map: &BTreeMap<String, String>) -> Result<Self> {
        let mut candidates = Vec::new();
        if let Some(path) = env_map
            .get(PSYCHEVO_GIT_BASH_PATH)
            .map(|value| value.trim())
            .filter(|value| !value.is_empty())
        {
            candidates.push(PathBuf::from(path));
        }
        if let Some(path) =
            find_on_path("git.exe", env_map).or_else(|| find_on_path("git", env_map))
        {
            candidates.extend(git_bash_candidates_from_git(&path));
        }
        candidates.extend(common_git_bash_candidates());
        if let Some(path) =
            find_on_path("bash.exe", env_map).or_else(|| find_on_path("bash", env_map))
        {
            candidates.push(path);
        }

        for bash in candidates {
            if let Some(runtime) = Self::from_bash_candidate(bash) {
                return Ok(runtime);
            }
        }

        Err(Error::Message(format!(
            "Git Bash is required for native Windows shell execution. Install Git for Windows or set {PSYCHEVO_GIT_BASH_PATH} to bash.exe."
        )))
    }

    pub fn from_bash_candidate(bash: PathBuf) -> Option<Self> {
        let cygpath = bash
            .parent()
            .map(|parent| parent.join("cygpath.exe"))
            .filter(|path| path.exists())
            .or_else(|| {
                bash.parent()
                    .and_then(|parent| parent.parent())
                    .map(|root| root.join("usr").join("bin").join("cygpath.exe"))
                    .filter(|path| path.exists())
            })?;
        Some(Self { bash, cygpath })
    }

    pub fn cygpath_windows(&self, raw: &str) -> Result<String> {
        let output = Command::new(&self.cygpath)
            .arg("-w")
            .arg("--")
            .arg(raw)
            .output()
            .map_err(|err| Error::Message(format!("failed to run cygpath: {err}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(Error::Message(format!(
                "failed to resolve Git Bash path with cygpath: {}",
                if stderr.is_empty() { raw } else { &stderr }
            )));
        }
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return Err(Error::Message(format!(
                "cygpath returned an empty path for {raw}"
            )));
        }
        Ok(value)
    }
}

pub fn path_ref_for_native_path(path: &Path) -> PathRef {
    let raw = path.to_string_lossy().to_string();
    if let Ok((native, convention)) = normalize_windows_absolute(&raw) {
        return windows_path_ref(&native, convention);
    }
    posix_path_ref(&normalize_posix_path(&raw))
}

pub fn normalized_native_path(path: &Path) -> PathBuf {
    path_ref_for_native_path(path).native_path()
}

pub fn display_path_for_native_path(path: &Path) -> String {
    path_ref_for_native_path(path).display
}

pub fn resolve_executable_path(
    command: &str,
    cwd: &Path,
    options: &ExecutableResolveOptions<'_>,
) -> Option<PathBuf> {
    let command = command.trim();
    if command.is_empty() {
        return None;
    }

    if command_has_path_separator(command) {
        let path = executable_command_path(command, cwd, options.platform);
        return executable_candidate(path, command_has_extension(command), options);
    }

    let path_value = env_value(options.env, "PATH")?;
    for entry in path_entries(path_value, options.platform) {
        let dir = path_entry_for_platform(&entry, options.platform);
        if let Some(path) = executable_candidate(dir.join(command), false, options) {
            return Some(path);
        }
    }
    None
}

pub fn resolve_host_path(
    raw: &str,
    cwd: &Path,
    options: &PathResolveOptions<'_>,
) -> Result<PathRef> {
    if raw.is_empty() {
        return Err(Error::Message("path must not be empty".to_string()));
    }
    if is_windows_drive_relative(raw) {
        return Err(Error::Message(format!(
            "drive-relative Windows paths are unsupported: {raw}"
        )));
    }
    if is_unsupported_windows_device_path(raw) {
        return Err(Error::Message(format!(
            "unsupported Windows device path: {raw}"
        )));
    }
    if let Some(path_ref) = parse_absolute_input(raw)? {
        return Ok(path_ref);
    }

    match options.platform {
        HostPlatform::Windows => {
            if raw.starts_with('/') {
                if options.shell_family != ShellFamily::GitBash {
                    return Err(Error::Message(format!(
                        "POSIX absolute path requires Git Bash on Windows: {raw}"
                    )));
                }
                let Some(cygpath) = options.cygpath else {
                    return Err(Error::Message(format!(
                        "Git Bash path requires cygpath resolution on Windows: {raw}"
                    )));
                };
                let native =
                    normalize_windows_absolute(&cygpath(raw)?).map(|(native, _)| native)?;
                return Ok(windows_path_ref(&native, PathConvention::GitBash));
            }
            let cwd_native = path_ref_for_native_path(cwd).native;
            let joined = join_windows_path(&cwd_native, raw);
            let native = normalize_windows_absolute(&joined).map(|(native, _)| native)?;
            Ok(windows_path_ref(&native, PathConvention::Windows))
        }
        HostPlatform::Posix => {
            let path = Path::new(raw);
            let joined = if path.is_absolute() {
                path.to_path_buf()
            } else {
                cwd.join(path)
            };
            Ok(posix_path_ref(&normalize_posix_path(
                &joined.to_string_lossy(),
            )))
        }
    }
}

pub fn resolve_input_path(raw: &str, cwd: &Path) -> Result<PathBuf> {
    if cfg!(windows) {
        let env_map = env::vars().collect::<BTreeMap<_, _>>();
        let git_bash = GitBashRuntime::discover(&env_map)?;
        let cygpath = |value: &str| git_bash.cygpath_windows(value);
        let options = PathResolveOptions {
            platform: HostPlatform::Windows,
            shell_family: ShellFamily::GitBash,
            cygpath: Some(&cygpath),
        };
        Ok(resolve_host_path(raw, cwd, &options)?.native_path())
    } else {
        Ok(resolve_host_path(raw, cwd, &PathResolveOptions::current())?.native_path())
    }
}

pub fn shell_is_git_bash(shell: &str) -> bool {
    let normalized = shell.replace('\\', "/").to_ascii_lowercase();
    normalized.ends_with("/bash.exe")
        || normalized.ends_with("/bash")
        || normalized == "bash.exe"
        || normalized == "bash"
}

fn executable_command_path(command: &str, cwd: &Path, platform: HostPlatform) -> PathBuf {
    if platform == HostPlatform::Windows {
        if let Ok(Some(path_ref)) = parse_absolute_input(command) {
            return path_ref.native_path();
        }
        if !Path::new(command).is_absolute() {
            let cwd_ref = path_ref_for_native_path(cwd);
            if normalize_windows_absolute(&cwd_ref.native).is_ok() {
                return PathBuf::from(join_windows_path(&cwd_ref.native, command));
            }
        }
    }

    let path = Path::new(command);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        cwd.join(path)
    }
}

fn executable_candidate(
    path: PathBuf,
    command_has_extension: bool,
    options: &ExecutableResolveOptions<'_>,
) -> Option<PathBuf> {
    match options.platform {
        HostPlatform::Posix => existing_file(path, options.platform),
        HostPlatform::Windows => {
            if command_has_extension {
                return existing_file(path, options.platform);
            }
            for extension in windows_pathext(options.env) {
                if let Some(path) = existing_file(
                    path_with_appended_extension(&path, &extension),
                    options.platform,
                ) {
                    return Some(path);
                }
            }
            existing_file(path, options.platform)
        }
    }
}

fn path_with_appended_extension(path: &Path, extension: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(extension);
    PathBuf::from(raw)
}

fn existing_file(path: PathBuf, platform: HostPlatform) -> Option<PathBuf> {
    if path.is_file() {
        return Some(path);
    }
    if platform == HostPlatform::Windows {
        return existing_file_case_insensitive(&path);
    }
    None
}

fn existing_file_case_insensitive(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?;
    let expected = path.file_name()?.to_string_lossy();
    for entry in fs::read_dir(parent).ok()? {
        let entry = entry.ok()?;
        let name = entry.file_name();
        if name.to_string_lossy().eq_ignore_ascii_case(&expected) && entry.path().is_file() {
            return Some(entry.path());
        }
    }
    None
}

fn path_entry_for_platform(raw: &str, platform: HostPlatform) -> PathBuf {
    if platform == HostPlatform::Windows
        && let Ok(Some(path_ref)) = parse_absolute_input(raw)
    {
        return path_ref.native_path();
    }
    PathBuf::from(raw)
}

fn path_entries(value: &str, platform: HostPlatform) -> Vec<String> {
    if platform == HostPlatform::Windows {
        let entries: Vec<_> = if value.contains(';') {
            value.split(';').collect()
        } else {
            split_git_bash_style_windows_path(value)
        };
        return entries
            .into_iter()
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .map(str::to_string)
            .collect();
    }
    env::split_paths(value)
        .map(|path| path.to_string_lossy().to_string())
        .collect()
}

fn split_git_bash_style_windows_path(value: &str) -> Vec<&str> {
    let mut entries = Vec::new();
    let mut start = 0;
    for (index, ch) in value.char_indices() {
        if ch == ':' && !is_windows_drive_colon(value, start, index) {
            entries.push(&value[start..index]);
            start = index + ch.len_utf8();
        }
    }
    entries.push(&value[start..]);
    entries
}

fn is_windows_drive_colon(value: &str, entry_start: usize, colon_index: usize) -> bool {
    if colon_index != entry_start + 1 {
        return false;
    }
    let Some(drive) = value[entry_start..colon_index].chars().next() else {
        return false;
    };
    if !drive.is_ascii_alphabetic() {
        return false;
    }
    matches!(value[colon_index + 1..].chars().next(), Some('\\' | '/'))
}

fn windows_pathext(env_map: &BTreeMap<String, String>) -> Vec<String> {
    let values = env_value(env_map, "PATHEXT")
        .map(|value| value.split(';').collect::<Vec<_>>())
        .filter(|values| values.iter().any(|value| !value.trim().is_empty()))
        .unwrap_or_else(|| vec![".COM", ".EXE", ".BAT", ".CMD"]);
    values
        .into_iter()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .flat_map(|value| {
            let extension = if value.starts_with('.') {
                value.to_string()
            } else {
                format!(".{value}")
            };
            let lower = extension.to_ascii_lowercase();
            if lower == extension {
                vec![extension]
            } else {
                vec![extension, lower]
            }
        })
        .collect()
}

fn env_value<'a>(env_map: &'a BTreeMap<String, String>, key: &str) -> Option<&'a str> {
    env_map
        .get(key)
        .or_else(|| {
            env_map
                .iter()
                .find(|(candidate, _)| candidate.eq_ignore_ascii_case(key))
                .map(|(_, value)| value)
        })
        .map(String::as_str)
}

fn command_has_path_separator(command: &str) -> bool {
    command.contains('/') || command.contains('\\')
}

fn command_has_extension(command: &str) -> bool {
    Path::new(command).extension().is_some()
}

fn parse_absolute_input(raw: &str) -> Result<Option<PathRef>> {
    if let Some(path_ref) = parse_file_uri(raw)? {
        return Ok(Some(path_ref));
    }
    if let Ok((native, convention)) = normalize_windows_absolute(raw) {
        return Ok(Some(windows_path_ref(&native, convention)));
    }
    if let Some(native) = msys_drive_path(raw) {
        return Ok(Some(windows_path_ref(&native, PathConvention::GitBash)));
    }
    if let Some(native) = cygwin_drive_path(raw) {
        return Ok(Some(windows_path_ref(&native, PathConvention::Cygwin)));
    }
    if let Some(native) = wsl_drive_path(raw) {
        return Ok(Some(windows_path_ref(&native, PathConvention::Wsl)));
    }
    Ok(None)
}

fn parse_file_uri(raw: &str) -> Result<Option<PathRef>> {
    let Some(rest) = raw.strip_prefix("file://") else {
        return Ok(None);
    };
    let decoded = percent_decode(rest)?;
    if decoded.starts_with('/') && decoded.len() >= 4 && is_drive_prefix(&decoded[1..3]) {
        let native = decoded[1..].replace('/', "\\");
        let native = normalize_windows_absolute(&native).map(|(native, _)| native)?;
        return Ok(Some(PathRef {
            convention: PathConvention::FileUri,
            ..windows_path_ref(&native, PathConvention::FileUri)
        }));
    }
    if !decoded.starts_with('/') {
        let native = format!("\\\\{}", decoded.replace('/', "\\"));
        let native = normalize_windows_absolute(&native).map(|(native, _)| native)?;
        return Ok(Some(PathRef {
            convention: PathConvention::FileUri,
            ..windows_path_ref(&native, PathConvention::FileUri)
        }));
    }
    Ok(Some(PathRef {
        convention: PathConvention::FileUri,
        ..posix_path_ref(&normalize_posix_path(&decoded))
    }))
}

fn normalize_windows_absolute(raw: &str) -> Result<(String, PathConvention)> {
    if raw.starts_with("\\\\.\\") || raw.starts_with("//./") {
        return Err(Error::Message(format!(
            "unsupported Windows device path: {raw}"
        )));
    }
    let simplified = if let Some(rest) = raw
        .strip_prefix("\\\\?\\UNC\\")
        .or_else(|| raw.strip_prefix("//?/UNC/"))
    {
        format!("\\\\{rest}")
    } else if let Some(rest) = raw
        .strip_prefix("\\\\?\\")
        .or_else(|| raw.strip_prefix("//?/"))
    {
        rest.to_string()
    } else {
        raw.to_string()
    };
    let value = simplified.replace('/', "\\");
    if value.len() >= 3 && is_drive_prefix(&value[..2]) && value.as_bytes()[2] == b'\\' {
        let drive = value[..1].to_ascii_uppercase();
        let rest = lexical_normalize_windows_segments(&value[3..]);
        let native = if rest.is_empty() {
            format!("{drive}:\\")
        } else {
            format!("{drive}:\\{rest}")
        };
        return Ok((native, PathConvention::Windows));
    }
    if value.starts_with("\\\\") {
        let trimmed = value.trim_start_matches('\\');
        let parts = trimmed
            .split('\\')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        if parts.len() < 2 {
            return Err(Error::Message(format!("invalid UNC path: {raw}")));
        }
        let rest = lexical_normalize_windows_segments(&parts[2..].join("\\"));
        let native = if rest.is_empty() {
            format!("\\\\{}\\{}", parts[0], parts[1])
        } else {
            format!("\\\\{}\\{}\\{rest}", parts[0], parts[1])
        };
        return Ok((native, PathConvention::Windows));
    }
    Err(Error::Message(format!(
        "not an absolute Windows path: {raw}"
    )))
}

fn msys_drive_path(raw: &str) -> Option<String> {
    let rest = raw.strip_prefix('/')?;
    let bytes = rest.as_bytes();
    if bytes.len() < 2 || !bytes[0].is_ascii_alphabetic() {
        return None;
    }
    let drive = (bytes[0] as char).to_ascii_uppercase();
    let after_drive = &rest[1..];
    if after_drive == ":" {
        return Some(format!("{drive}:\\"));
    }
    let after = after_drive
        .strip_prefix(":/")
        .or_else(|| after_drive.strip_prefix('/'))?;
    let suffix = after.replace('/', "\\");
    Some(if suffix.is_empty() {
        format!("{drive}:\\")
    } else {
        format!("{drive}:\\{suffix}")
    })
}

fn cygwin_drive_path(raw: &str) -> Option<String> {
    let rest = raw.strip_prefix("/cygdrive/")?;
    prefixed_drive_path(rest)
}

fn wsl_drive_path(raw: &str) -> Option<String> {
    let rest = raw.strip_prefix("/mnt/")?;
    prefixed_drive_path(rest)
}

fn prefixed_drive_path(rest: &str) -> Option<String> {
    let mut chars = rest.chars();
    let drive = chars.next()?.to_ascii_uppercase();
    if !drive.is_ascii_alphabetic() {
        return None;
    }
    let after = chars.as_str().strip_prefix('/').unwrap_or(chars.as_str());
    let suffix = after.replace('/', "\\");
    Some(if suffix.is_empty() {
        format!("{drive}:\\")
    } else {
        format!("{drive}:\\{suffix}")
    })
}

fn windows_path_ref(native: &str, convention: PathConvention) -> PathRef {
    let native = normalize_windows_absolute(native)
        .map(|(value, _)| value)
        .unwrap_or_else(|_| native.to_string());
    PathRef {
        uri: windows_file_uri(&native),
        display: windows_git_bash_display(&native),
        native,
        convention,
    }
}

fn posix_path_ref(native: &str) -> PathRef {
    let native = normalize_posix_path(native);
    PathRef {
        uri: posix_file_uri(&native),
        display: native.clone(),
        native,
        convention: PathConvention::Posix,
    }
}

fn windows_file_uri(native: &str) -> String {
    if native.starts_with("\\\\") {
        let trimmed = native.trim_start_matches('\\');
        let parts = trimmed.split('\\').collect::<Vec<_>>();
        if parts.len() >= 2 {
            let path = parts
                .iter()
                .skip(2)
                .map(|part| percent_encode(part))
                .collect::<Vec<_>>()
                .join("/");
            if path.is_empty() {
                return format!("file://{}/{}", parts[0], parts[1]);
            }
            return format!("file://{}/{}/{}", parts[0], parts[1], path);
        }
    }
    let value = native.replace('\\', "/");
    format!("file:///{}", percent_encode_path(&value))
}

fn posix_file_uri(native: &str) -> String {
    format!("file://{}", percent_encode_path(native))
}

fn windows_git_bash_display(native: &str) -> String {
    if native.len() >= 3 && is_drive_prefix(&native[..2]) {
        let drive = native[..1].to_ascii_lowercase();
        let rest = native[3..].replace('\\', "/");
        if rest.is_empty() {
            format!("/{drive}")
        } else {
            format!("/{drive}/{rest}")
        }
    } else if native.starts_with("\\\\") {
        format!("//{}", native.trim_start_matches('\\').replace('\\', "/"))
    } else {
        native.replace('\\', "/")
    }
}

fn join_windows_path(cwd: &str, raw: &str) -> String {
    let separator = if cwd.ends_with('\\') || cwd.ends_with('/') {
        ""
    } else {
        "\\"
    };
    format!("{cwd}{separator}{}", raw.replace('/', "\\"))
}

fn normalize_posix_path(raw: &str) -> String {
    let absolute = raw.starts_with('/');
    let mut out = Vec::new();
    for segment in raw.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    let mut value = out.join("/");
    if absolute {
        value.insert(0, '/');
    }
    if value.is_empty() {
        if absolute {
            "/".to_string()
        } else {
            ".".to_string()
        }
    } else {
        value
    }
}

fn lexical_normalize_windows_segments(raw: &str) -> String {
    let mut out: Vec<&str> = Vec::new();
    for segment in raw.split('\\') {
        match segment {
            "" | "." => {}
            ".." => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out.join("\\")
}

fn is_drive_prefix(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() == 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

fn is_windows_drive_relative(raw: &str) -> bool {
    let bytes = raw.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && !matches!(bytes.get(2), Some(b'\\' | b'/'))
}

fn is_unsupported_windows_device_path(raw: &str) -> bool {
    raw.starts_with("\\\\.\\") || raw.starts_with("//./")
}

fn percent_encode_path(path: &str) -> String {
    path.split('/')
        .map(percent_encode)
        .collect::<Vec<_>>()
        .join("/")
}

fn percent_encode(value: &str) -> String {
    let mut out = String::new();
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b':' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

fn percent_decode(value: &str) -> Result<String> {
    let mut out = Vec::new();
    let bytes = value.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let Some(hex) = bytes.get(index + 1..index + 3) else {
                return Err(Error::Message(format!("invalid file URI escape: {value}")));
            };
            let text = std::str::from_utf8(hex)
                .map_err(|_| Error::Message(format!("invalid file URI escape: {value}")))?;
            let byte = u8::from_str_radix(text, 16)
                .map_err(|_| Error::Message(format!("invalid file URI escape: {value}")))?;
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(out).map_err(|_| Error::Message(format!("invalid file URI: {value}")))
}

fn find_on_path(name: &str, env_map: &BTreeMap<String, String>) -> Option<PathBuf> {
    let path = env_map.get("PATH").or_else(|| env_map.get("Path"))?;
    for dir in env::split_paths(path) {
        let candidate = dir.join(name);
        if candidate.exists() {
            return Some(candidate);
        }
    }
    None
}

fn git_bash_candidates_from_git(git: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(parent) = git.parent() {
        out.push(parent.join("bash.exe"));
        if let Some(root) = parent.parent() {
            out.push(root.join("bin").join("bash.exe"));
            out.push(root.join("usr").join("bin").join("bash.exe"));
        }
    }
    out
}

fn common_git_bash_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from(r"C:\Program Files\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files\Git\usr\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\bin\bash.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Git\usr\bin\bash.exe"),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn windows_options<'a>(cygpath: Option<CygpathResolver<'a>>) -> PathResolveOptions<'a> {
        PathResolveOptions {
            platform: HostPlatform::Windows,
            shell_family: ShellFamily::GitBash,
            cygpath,
        }
    }

    #[test]
    fn parses_windows_drive_path_to_git_bash_display_and_uri() {
        let cwd = Path::new(r"C:\repo");
        let path =
            resolve_host_path(r"C:\Users\Ada\project", cwd, &windows_options(None)).expect("path");
        assert_eq!(path.native, r"C:\Users\Ada\project");
        assert_eq!(path.display, "/c/Users/Ada/project");
        assert_eq!(path.uri, "file:///C:/Users/Ada/project");
        assert_eq!(path.convention, PathConvention::Windows);
    }

    #[test]
    fn parses_git_bash_cygwin_and_wsl_drive_paths() {
        let cwd = Path::new(r"C:\repo");
        for (raw, convention) in [
            ("/c/Users/Ada/project", PathConvention::GitBash),
            ("/c:/Users/Ada/project", PathConvention::GitBash),
            ("/cygdrive/c/Users/Ada/project", PathConvention::Cygwin),
            ("/mnt/c/Users/Ada/project", PathConvention::Wsl),
        ] {
            let path = resolve_host_path(raw, cwd, &windows_options(None)).expect(raw);
            assert_eq!(path.native, r"C:\Users\Ada\project");
            assert_eq!(path.display, "/c/Users/Ada/project");
            assert_eq!(path.uri, "file:///C:/Users/Ada/project");
            assert_eq!(path.convention, convention);
        }
    }

    #[test]
    fn parses_unc_and_verbatim_paths() {
        let cwd = Path::new(r"C:\repo");
        let unc =
            resolve_host_path(r"\\server\share\dir", cwd, &windows_options(None)).expect("unc");
        assert_eq!(unc.native, r"\\server\share\dir");
        assert_eq!(unc.display, "//server/share/dir");
        assert_eq!(unc.uri, "file://server/share/dir");

        let verbatim = resolve_host_path(r"\\?\C:\repo\file.txt", cwd, &windows_options(None))
            .expect("verbatim");
        assert_eq!(verbatim.native, r"C:\repo\file.txt");
        assert_eq!(verbatim.display, "/c/repo/file.txt");
        assert_eq!(verbatim.uri, "file:///C:/repo/file.txt");

        let verbatim_unc =
            resolve_host_path(r"\\?\UNC\server\share\dir", cwd, &windows_options(None))
                .expect("verbatim unc");
        assert_eq!(verbatim_unc.native, r"\\server\share\dir");
        assert_eq!(verbatim_unc.display, "//server/share/dir");
        assert_eq!(verbatim_unc.uri, "file://server/share/dir");
    }

    #[test]
    fn path_ref_normalizes_windows_verbatim_native_paths() {
        let drive = path_ref_for_native_path(Path::new(r"\\?\C:\Users\Ada\project"));
        assert_eq!(drive.native, r"C:\Users\Ada\project");
        assert_eq!(drive.display, "/c/Users/Ada/project");
        assert_eq!(drive.uri, "file:///C:/Users/Ada/project");

        let slash_drive = path_ref_for_native_path(Path::new("//?/C:/Users/Ada/project"));
        assert_eq!(slash_drive.native, r"C:\Users\Ada\project");
        assert_eq!(slash_drive.display, "/c/Users/Ada/project");
        assert_eq!(slash_drive.uri, "file:///C:/Users/Ada/project");

        let unc = path_ref_for_native_path(Path::new(r"\\?\UNC\server\share\project"));
        assert_eq!(unc.native, r"\\server\share\project");
        assert_eq!(unc.display, "//server/share/project");
        assert_eq!(unc.uri, "file://server/share/project");
    }

    #[test]
    fn windows_path_entries_preserve_drive_colons_and_split_git_bash_paths() {
        assert_eq!(
            path_entries(r"C:\Tools", HostPlatform::Windows),
            [r"C:\Tools"]
        );
        assert_eq!(
            path_entries(r"C:\Tools;D:\Bin", HostPlatform::Windows),
            [r"C:\Tools", r"D:\Bin"]
        );
        assert_eq!(
            path_entries("/c/Tools:/usr/bin", HostPlatform::Windows),
            ["/c/Tools", "/usr/bin"]
        );
        assert_eq!(
            path_entries("C:/Tools:/usr/bin", HostPlatform::Windows),
            ["C:/Tools", "/usr/bin"]
        );
    }

    #[test]
    fn resolves_windows_pathext_command_shims() {
        let temp = tempfile::tempdir().expect("temp");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        let shim = bin.join("opencode.cmd");
        std::fs::write(&shim, "@echo off\n").expect("shim");
        let env = BTreeMap::from([("PATH".to_string(), bin.display().to_string())]);

        let resolved = resolve_executable_path(
            "opencode",
            temp.path(),
            &ExecutableResolveOptions {
                platform: HostPlatform::Windows,
                env: &env,
            },
        )
        .expect("resolved");

        assert_eq!(resolved, shim);
    }

    #[test]
    fn resolves_windows_executable_before_extensionless_shell_shim() {
        let temp = tempfile::tempdir().expect("temp");
        let bin = temp.path().join("bin");
        std::fs::create_dir_all(&bin).expect("bin");
        std::fs::write(bin.join("opencode"), "#!/bin/sh\n").expect("shell shim");
        let exe = bin.join("opencode.exe");
        std::fs::write(&exe, "").expect("exe");
        let env = BTreeMap::from([("PATH".to_string(), bin.display().to_string())]);

        let resolved = resolve_executable_path(
            "opencode",
            temp.path(),
            &ExecutableResolveOptions {
                platform: HostPlatform::Windows,
                env: &env,
            },
        )
        .expect("resolved");

        assert_eq!(resolved, exe);
    }

    #[test]
    fn resolves_configured_relative_executable_with_windows_pathext() {
        let temp = tempfile::tempdir().expect("temp");
        let tools = temp.path().join("tools");
        std::fs::create_dir_all(&tools).expect("tools");
        let shim = tools.join("opencode.cmd");
        std::fs::write(&shim, "@echo off\n").expect("shim");
        let env = BTreeMap::new();

        let resolved = resolve_executable_path(
            "tools/opencode",
            temp.path(),
            &ExecutableResolveOptions {
                platform: HostPlatform::Windows,
                env: &env,
            },
        )
        .expect("resolved");

        assert_eq!(resolved, shim);
    }

    #[test]
    fn unresolved_executable_returns_none() {
        let temp = tempfile::tempdir().expect("temp");
        let env = BTreeMap::new();

        let resolved = resolve_executable_path(
            "opencode",
            temp.path(),
            &ExecutableResolveOptions {
                platform: HostPlatform::Windows,
                env: &env,
            },
        );

        assert!(resolved.is_none());
    }

    #[test]
    fn rejects_drive_relative_and_device_paths() {
        let cwd = Path::new(r"C:\repo");
        assert!(resolve_host_path("C:repo", cwd, &windows_options(None)).is_err());
        assert!(resolve_host_path(r"\\.\NUL", cwd, &windows_options(None)).is_err());
    }

    #[test]
    fn resolves_git_bash_virtual_paths_with_cygpath() {
        let cwd = Path::new(r"C:\repo");
        let cygpath = |value: &str| {
            assert_eq!(value, "/tmp");
            Ok(r"C:\Users\Ada\AppData\Local\Temp".to_string())
        };
        let path = resolve_host_path("/tmp", cwd, &windows_options(Some(&cygpath))).expect("tmp");
        assert_eq!(path.native, r"C:\Users\Ada\AppData\Local\Temp");
        assert_eq!(path.display, "/c/Users/Ada/AppData/Local/Temp");
        assert_eq!(path.convention, PathConvention::GitBash);
    }

    #[test]
    fn joins_relative_paths_using_windows_cwd() {
        let cwd = Path::new(r"C:\repo");
        let path = resolve_host_path("src/lib.rs", cwd, &windows_options(None)).expect("relative");
        assert_eq!(path.native, r"C:\repo\src\lib.rs");
        assert_eq!(path.uri, "file:///C:/repo/src/lib.rs");
    }

    #[test]
    fn preserves_posix_relative_path_whitespace() {
        let path = resolve_host_path(
            "  spaced  ",
            Path::new("/repo"),
            &PathResolveOptions::current(),
        )
        .expect("relative");

        assert_eq!(path.native, "/repo/  spaced  ");
        assert_eq!(path.display, "/repo/  spaced  ");
    }

    #[test]
    fn preserves_posix_absolute_path_whitespace() {
        let path = resolve_host_path(
            "/tmp/ leading /trail ",
            Path::new("/repo"),
            &PathResolveOptions::current(),
        )
        .expect("absolute");

        assert_eq!(path.native, "/tmp/ leading /trail ");
        assert_eq!(path.uri, "file:///tmp/%20leading%20/trail%20");
    }

    #[test]
    fn preserves_windows_absolute_path_whitespace() {
        let cwd = Path::new(r"C:\repo");
        let path = resolve_host_path(r"C:\repo\ file ", cwd, &windows_options(None))
            .expect("windows path");

        assert_eq!(path.native, r"C:\repo\ file ");
        assert_eq!(path.uri, "file:///C:/repo/%20file%20");
    }

    #[test]
    fn parses_file_uris() {
        let cwd = Path::new(r"C:\repo");
        let drive = resolve_host_path("file:///C:/repo/a%20b.txt", cwd, &windows_options(None))
            .expect("drive uri");
        assert_eq!(drive.native, r"C:\repo\a b.txt");
        assert_eq!(drive.convention, PathConvention::FileUri);

        let posix = resolve_host_path(
            "file:///tmp/a%20b.txt",
            Path::new("/repo"),
            &PathResolveOptions::current(),
        )
        .expect("posix uri");
        assert_eq!(posix.native, "/tmp/a b.txt");
        assert_eq!(posix.uri, "file:///tmp/a%20b.txt");
    }
}
