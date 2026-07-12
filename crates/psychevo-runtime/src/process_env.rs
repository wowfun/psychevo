use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use chardetng::EncodingDetector;
use encoding_rs::{Encoding, GBK, IBM866, WINDOWS_1251, WINDOWS_1252};

use crate::{Error, HostPlatform, Result};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostProcessLaunch {
    pub(crate) program: PathBuf,
    pub(crate) args: Vec<OsString>,
    pub(crate) windows_raw_arg: Option<OsString>,
}

pub(crate) fn host_process_launch(
    program: &Path,
    args: &[OsString],
    platform: HostPlatform,
    env_map: &BTreeMap<String, String>,
) -> Result<HostProcessLaunch> {
    if platform != HostPlatform::Windows || !is_windows_command_script(program) {
        return Ok(HostProcessLaunch {
            program: program.to_path_buf(),
            args: args.to_vec(),
            windows_raw_arg: None,
        });
    }
    let command_processor = env_value_case_insensitive(env_map, "COMSPEC")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cmd.exe"));
    Ok(HostProcessLaunch {
        program: command_processor,
        args: vec![
            OsString::from("/D"),
            OsString::from("/S"),
            OsString::from("/V:OFF"),
            OsString::from("/C"),
        ],
        windows_raw_arg: Some(OsString::from(windows_command_script_line(program, args)?)),
    })
}

pub fn tokio_host_process_command(
    program: &Path,
    args: &[OsString],
    platform: HostPlatform,
    env_map: &BTreeMap<String, String>,
) -> Result<tokio::process::Command> {
    let launch = host_process_launch(program, args, platform, env_map)?;
    let mut command = tokio::process::Command::new(launch.program);
    command.args(launch.args);
    if let Some(raw_arg) = launch.windows_raw_arg {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.as_std_mut().raw_arg(raw_arg);
        }
        #[cfg(not(windows))]
        {
            // Preserve a deterministic command shape for Windows-platform
            // tests running on other hosts. Windows builds use raw_arg above.
            command.arg(raw_arg);
        }
    }
    Ok(command)
}

fn is_windows_command_script(program: &Path) -> bool {
    program
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            extension.eq_ignore_ascii_case("cmd") || extension.eq_ignore_ascii_case("bat")
        })
}

fn windows_command_script_line(program: &Path, args: &[OsString]) -> Result<String> {
    let mut command_line = String::from("\"");
    command_line.push_str(&windows_command_script_token(program.as_os_str())?);
    for arg in args {
        command_line.push(' ');
        command_line.push_str(&windows_command_script_token(arg)?);
    }
    command_line.push('"');
    Ok(command_line)
}

fn windows_command_script_token(value: &std::ffi::OsStr) -> Result<String> {
    let value = value.to_str().ok_or_else(|| {
        Error::Message("Windows command-script paths and arguments must be Unicode".to_string())
    })?;
    if value.contains(['\0', '\r', '\n', '"', '%']) {
        return Err(Error::Message(
            "Windows command-script paths and arguments cannot contain NUL, newlines, quotes, or percent expansion"
                .to_string(),
        ));
    }
    Ok(format!("\"{value}\""))
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessEnvOptions<'a> {
    pub path_prefixes: &'a [PathBuf],
    pub windows_utf8_defaults: bool,
}

impl<'a> ProcessEnvOptions<'a> {
    pub fn new(path_prefixes: &'a [PathBuf]) -> Self {
        Self {
            path_prefixes,
            windows_utf8_defaults: cfg!(windows),
        }
    }

    pub fn with_windows_utf8_defaults(mut self, value: bool) -> Self {
        self.windows_utf8_defaults = value;
        self
    }
}

pub fn effective_process_env(
    env_map: &BTreeMap<String, String>,
    options: ProcessEnvOptions<'_>,
) -> Result<BTreeMap<String, String>> {
    let mut env = env_map.clone();
    if options.windows_utf8_defaults {
        for (key, value) in windows_utf8_default_env(&env) {
            env.insert(key.to_string(), value.to_string());
        }
    }
    if let Some((key, value)) = prefixed_path_overlay(options.path_prefixes, env_map)? {
        env.insert(key, value.to_string_lossy().to_string());
    }
    Ok(env)
}

pub fn apply_process_env(
    command: &mut Command,
    env_map: &BTreeMap<String, String>,
    options: ProcessEnvOptions<'_>,
) -> Result<()> {
    for (key, value) in effective_process_env(env_map, options)? {
        command.env(key, value);
    }
    Ok(())
}

pub fn apply_tokio_process_env(
    command: &mut tokio::process::Command,
    env_map: &BTreeMap<String, String>,
    options: ProcessEnvOptions<'_>,
) -> Result<()> {
    for (key, value) in effective_process_env(env_map, options)? {
        command.env(key, value);
    }
    Ok(())
}

pub fn apply_pty_process_env(
    command: &mut portable_pty::CommandBuilder,
    env_map: &BTreeMap<String, String>,
    options: ProcessEnvOptions<'_>,
) -> Result<()> {
    for (key, value) in effective_process_env(env_map, options)? {
        command.env(key, value);
    }
    Ok(())
}

pub fn windows_utf8_default_env(
    env_map: &BTreeMap<String, String>,
) -> Vec<(&'static str, &'static str)> {
    let mut defaults = Vec::new();
    if env_value_case_insensitive(env_map, "PYTHONUTF8").is_none() {
        defaults.push(("PYTHONUTF8", "1"));
    }
    if env_value_case_insensitive(env_map, "PYTHONIOENCODING").is_none() {
        defaults.push(("PYTHONIOENCODING", "utf-8"));
    }
    if env_value_case_insensitive(env_map, "LANG").is_none() {
        defaults.push(("LANG", "C.UTF-8"));
    }
    if env_value_case_insensitive(env_map, "LC_ALL").is_none() {
        defaults.push(("LC_ALL", "C.UTF-8"));
    }
    if env_value_case_insensitive(env_map, "LC_CTYPE").is_none() {
        defaults.push(("LC_CTYPE", "C.UTF-8"));
    }
    defaults
}

pub fn env_value_case_insensitive<'a>(
    env_map: &'a BTreeMap<String, String>,
    key: &str,
) -> Option<&'a str> {
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

pub fn combined_path_value(
    env_map: &BTreeMap<String, String>,
    path_prefixes: &[PathBuf],
) -> Result<Option<OsString>> {
    let mut paths = path_prefixes.to_vec();
    if let Some(current) = env_value_case_insensitive(env_map, "PATH") {
        paths.extend(std::env::split_paths(current));
    } else if let Some(current) = std::env::var_os("PATH") {
        paths.extend(std::env::split_paths(&current));
    }
    if paths.is_empty() {
        return Ok(None);
    }
    std::env::join_paths(paths)
        .map(Some)
        .map_err(|err| Error::Message(format!("failed to build subprocess PATH: {err}")))
}

pub fn prefixed_path_overlay(
    path_prefixes: &[PathBuf],
    env_map: &BTreeMap<String, String>,
) -> Result<Option<(String, OsString)>> {
    if path_prefixes.is_empty() {
        return Ok(None);
    }
    let key = env_key_case_insensitive(env_map, "PATH").unwrap_or_else(|| "PATH".to_string());
    combined_path_value(env_map, path_prefixes).map(|path| path.map(|path| (key, path)))
}

fn env_key_case_insensitive(env_map: &BTreeMap<String, String>, key: &str) -> Option<String> {
    if env_map.contains_key(key) {
        return Some(key.to_string());
    }
    env_map
        .keys()
        .find(|candidate| candidate.eq_ignore_ascii_case(key))
        .cloned()
}

pub fn windows_pathext_extensions(env_map: &BTreeMap<String, String>) -> Vec<String> {
    let values = env_value_case_insensitive(env_map, "PATHEXT")
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

pub fn executable_path_candidates(
    path: &Path,
    command_has_extension: bool,
    env_map: &BTreeMap<String, String>,
    windows_platform: bool,
) -> Vec<PathBuf> {
    if !windows_platform || command_has_extension {
        return vec![path.to_path_buf()];
    }
    let mut candidates = windows_pathext_extensions(env_map)
        .into_iter()
        .map(|extension| path_with_appended_extension(path, &extension))
        .collect::<Vec<_>>();
    candidates.push(path.to_path_buf());
    candidates
}

fn path_with_appended_extension(path: &Path, extension: &str) -> PathBuf {
    let mut raw = path.as_os_str().to_os_string();
    raw.push(extension);
    PathBuf::from(raw)
}

pub fn decode_process_output(bytes: &[u8]) -> String {
    decode_process_output_for_platform(bytes, cfg!(windows))
}

pub fn decode_process_output_for_platform(bytes: &[u8], windows_locale_fallback: bool) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(output) = std::str::from_utf8(bytes) {
        return output.to_string();
    }
    if windows_locale_fallback && let Some(output) = decode_windows_legacy(bytes) {
        return output;
    }
    String::from_utf8_lossy(bytes).to_string()
}

fn decode_windows_legacy(bytes: &[u8]) -> Option<String> {
    if looks_like_windows_1252_punctuation(bytes) {
        return decode_without_errors(bytes, WINDOWS_1252);
    }
    let encoding = detect_encoding(bytes);
    if let Some(decoded) = decode_without_errors(bytes, encoding)
        && detected_legacy_decoding_is_plausible(encoding, &decoded)
    {
        return Some(decoded);
    }
    if let Some(decoded) = decode_gb18030_without_errors(bytes)
        && decoded.chars().any(is_cjk_char)
    {
        return Some(decoded);
    }
    None
}

fn detected_legacy_decoding_is_plausible(encoding: &'static Encoding, decoded: &str) -> bool {
    if encoding == GBK {
        return decoded.chars().any(is_cjk_char);
    }
    if encoding == IBM866 || encoding == WINDOWS_1251 {
        return decoded.chars().any(is_cyrillic_char);
    }
    false
}

fn detect_encoding(bytes: &[u8]) -> &'static Encoding {
    let mut detector = EncodingDetector::new();
    detector.feed(bytes, true);
    let (encoding, _confidence) = detector.guess_assess(None, true);
    if encoding == IBM866 && looks_like_windows_1252_punctuation(bytes) {
        return WINDOWS_1252;
    }
    encoding
}

fn decode_without_errors(bytes: &[u8], encoding: &'static Encoding) -> Option<String> {
    let (decoded, _, had_errors) = encoding.decode(bytes);
    (!had_errors).then(|| decoded.into_owned())
}

fn decode_gb18030_without_errors(bytes: &[u8]) -> Option<String> {
    let (decoded, _, had_errors) = encoding_rs2::GB18030.decode(bytes);
    (!had_errors).then(|| decoded.into_owned())
}

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x20000..=0x2A6DF
            | 0x2A700..=0x2B73F
            | 0x2B740..=0x2B81F
            | 0x2B820..=0x2CEAF
            | 0x2CEB0..=0x2EBEF
            | 0x30000..=0x3134F
    )
}

fn is_cyrillic_char(ch: char) -> bool {
    matches!(ch as u32, 0x0400..=0x052F)
}

const WINDOWS_1252_PUNCT_BYTES: [u8; 8] = [0x91, 0x92, 0x93, 0x94, 0x95, 0x96, 0x97, 0x99];

fn looks_like_windows_1252_punctuation(bytes: &[u8]) -> bool {
    let mut saw_extended_punctuation = false;
    let mut saw_ascii_word = false;
    for &byte in bytes {
        if byte >= 0xA0 {
            return false;
        }
        if (0x80..=0x9F).contains(&byte) {
            if !WINDOWS_1252_PUNCT_BYTES.contains(&byte) {
                return false;
            }
            saw_extended_punctuation = true;
        }
        if byte.is_ascii_alphabetic() {
            saw_ascii_word = true;
        }
    }
    saw_extended_punctuation && saw_ascii_word
}

pub fn terminate_std_child_tree(child: &mut std::process::Child) {
    #[cfg(windows)]
    {
        let _ = kill_windows_process_tree(child.id());
    }
    let _ = child.kill();
}

pub fn terminate_std_child_process_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = kill_process_group_by_pid(child.id());
    }
    #[cfg(windows)]
    {
        let _ = kill_windows_process_tree(child.id());
    }
    let _ = child.kill();
}

pub async fn terminate_tokio_child_tree(child: &mut tokio::process::Child) {
    #[cfg(windows)]
    if let Some(pid) = child.id() {
        let _ = kill_windows_process_tree(pid);
    }
    let _ = child.kill().await;
}

pub fn terminate_pty_child_tree(child: &mut dyn portable_pty::Child) {
    #[cfg(windows)]
    if let Some(pid) = child.process_id() {
        let _ = kill_windows_process_tree(pid);
    }
    let _ = child.kill();
}

#[cfg(unix)]
pub fn kill_process_group_by_pid(pid: u32) -> std::io::Result<()> {
    let pgid = unsafe { libc::getpgid(pid as libc::pid_t) };
    if pgid == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err);
        }
        return Ok(());
    }
    let result = unsafe { libc::killpg(pgid, libc::SIGKILL) };
    if result == -1 {
        let err = std::io::Error::last_os_error();
        if err.kind() != std::io::ErrorKind::NotFound {
            return Err(err);
        }
    }
    Ok(())
}

#[cfg(any(windows, test))]
pub fn windows_taskkill_args(pid: u32) -> [String; 4] {
    [
        "/PID".to_string(),
        pid.to_string(),
        "/T".to_string(),
        "/F".to_string(),
    ]
}

#[cfg(windows)]
pub fn kill_windows_process_tree(pid: u32) -> bool {
    match Command::new("taskkill")
        .args(windows_taskkill_args(pid))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(status) => status.success(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_utf8_defaults_include_locale_and_preserve_explicit_values() {
        let defaults = windows_utf8_default_env(&BTreeMap::from([
            ("PYTHONIOENCODING".to_string(), "utf-16".to_string()),
            ("lc_ctype".to_string(), "C".to_string()),
        ]));

        assert!(defaults.iter().any(|(key, _)| *key == "PYTHONUTF8"));
        assert!(defaults.iter().any(|(key, _)| *key == "LANG"));
        assert!(defaults.iter().any(|(key, _)| *key == "LC_ALL"));
        assert!(
            defaults
                .iter()
                .all(|(key, _)| !key.eq_ignore_ascii_case("PYTHONIOENCODING"))
        );
        assert!(
            defaults
                .iter()
                .all(|(key, _)| !key.eq_ignore_ascii_case("LC_CTYPE"))
        );
    }

    #[test]
    fn path_overlay_preserves_existing_path_key_case_and_prefix_order() {
        let temp = tempfile::tempdir().expect("temp");
        let tools = temp.path().join("tools");
        let inherited = temp.path().join("inherited");
        std::fs::create_dir_all(&tools).expect("tools");
        std::fs::create_dir_all(&inherited).expect("inherited");
        let env = BTreeMap::from([("Path".to_string(), inherited.display().to_string())]);

        let (key, path) = prefixed_path_overlay(std::slice::from_ref(&tools), &env)
            .expect("path")
            .expect("path overlay");
        let entries = std::env::split_paths(&path).collect::<Vec<_>>();

        assert_eq!(key, "Path");
        assert_eq!(entries.first(), Some(&tools));
        assert_eq!(entries.get(1), Some(&inherited));
    }

    #[test]
    fn pathext_candidates_use_configured_extensions_before_extensionless_path() {
        let env = BTreeMap::from([("pathext".to_string(), "cmd;.EXE".to_string())]);
        let candidates = executable_path_candidates(Path::new("tool"), false, &env, true);

        assert_eq!(
            candidates,
            [
                PathBuf::from("tool.cmd"),
                PathBuf::from("tool.EXE"),
                PathBuf::from("tool.exe"),
                PathBuf::from("tool")
            ]
        );
    }

    #[test]
    fn windows_command_scripts_use_captured_comspec_with_one_quoted_command_line() {
        let env = BTreeMap::from([(
            "ComSpec".to_string(),
            r"C:\Windows\System32\cmd.exe".to_string(),
        )]);
        let launch = host_process_launch(
            Path::new(r"C:\Program Files\nodejs\npm.cmd"),
            &[OsString::from("ci"), OsString::from("--omit=dev")],
            HostPlatform::Windows,
            &env,
        )
        .expect("Windows command-script launch");

        assert_eq!(
            launch.program,
            PathBuf::from(r"C:\Windows\System32\cmd.exe")
        );
        assert_eq!(
            launch.args,
            vec![
                OsString::from("/D"),
                OsString::from("/S"),
                OsString::from("/V:OFF"),
                OsString::from("/C"),
            ]
        );
        assert_eq!(
            launch.windows_raw_arg,
            Some(OsString::from(
                r#"""C:\Program Files\nodejs\npm.cmd" "ci" "--omit=dev"""#,
            ))
        );
    }

    #[test]
    fn windows_executables_and_posix_programs_bypass_command_processor() {
        let args = [OsString::from("--version")];
        for (program, platform) in [
            (Path::new(r"C:\Tools\node.exe"), HostPlatform::Windows),
            (Path::new("/usr/bin/npm"), HostPlatform::Posix),
        ] {
            let launch = host_process_launch(program, &args, platform, &BTreeMap::new())
                .expect("direct launch");
            assert_eq!(launch.program, program);
            assert_eq!(launch.args, args);
            assert_eq!(launch.windows_raw_arg, None);
        }
    }

    #[test]
    fn windows_command_script_launch_rejects_percent_expansion() {
        let error = host_process_launch(
            Path::new(r"C:\Tools\npm.cmd"),
            &[OsString::from("%UNTRUSTED%")],
            HostPlatform::Windows,
            &BTreeMap::new(),
        )
        .expect_err("percent expansion must be rejected");

        assert!(error.to_string().contains("percent expansion"), "{error}");
    }

    #[test]
    fn process_output_decodes_utf8_and_windows_legacy_bytes() {
        assert_eq!(
            decode_process_output_for_platform("中文".as_bytes(), true),
            "中文"
        );
        assert_eq!(
            decode_process_output_for_platform(&[0xD6, 0xD0, 0xCE, 0xC4], true),
            "中文"
        );
        assert_eq!(
            decode_process_output_for_platform(&[0x95, 0x32, 0x82, 0x36], true),
            "𠀀"
        );
        assert_eq!(
            decode_process_output_for_platform(b"\x93\x94 test \x96 dash", true),
            "\u{201C}\u{201D} test \u{2013} dash"
        );
        assert_eq!(
            decode_process_output_for_platform(b"\xEF\xF0\xE8\xEC\xE5\xF0", true),
            "пример"
        );
        assert_eq!(
            decode_process_output_for_platform(b"\xAF\xE0\xA8\xAC\xA5\xE0", true),
            "пример"
        );
    }

    #[test]
    fn process_output_invalid_bytes_fall_back_to_lossy_text() {
        let output = decode_process_output_for_platform(&[0xFF, 0xFF], true);

        assert!(!output.is_empty());
        assert!(output.contains('\u{FFFD}'), "{output:?}");
    }

    #[test]
    fn windows_taskkill_args_target_process_tree() {
        assert_eq!(
            windows_taskkill_args(42),
            [
                "/PID".to_string(),
                "42".to_string(),
                "/T".to_string(),
                "/F".to_string()
            ]
        );
    }
}
