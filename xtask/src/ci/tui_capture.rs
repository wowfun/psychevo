use std::collections::HashSet;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result, bail};

use super::process::{
    LoggedChild, ProcessOutcome, command_exists, run_logged_process, write_log_line,
    write_mirrored_line,
};

const TUI_CAPTURE_SCREENSHOTS: &[&str] = &[
    "01-model-picker.png",
    "19-diff-overlay.png",
    "20-inline-edit-diff.png",
    "21-permission-approval.png",
    "02-running-thinking.png",
    "03-final-ledger.png",
    "04-shell-mode.png",
    "05-long-markdown-bottom-scroll.png",
    "06-reasoning-only-collapsed.png",
    "07-reasoning-only-bottom-scroll.png",
    "08-visible-write-preamble.png",
    "09-interrupted-exec-command.png",
    "16-clarify-panel.png",
    "17-clarify-other-inline.png",
    "18-clarify-result.png",
    "22-agent-background-handoff.png",
    "10-agent-tool-running.png",
    "11-agent-session-running.png",
    "12-agent-parent-completed.png",
    "12-agents-running.png",
    "13-agents-available.png",
    "14-agent-actions.png",
    "15-agent-run-prompt.png",
];

const TUI_CAPTURE_DEPS: &[&str] = &["vhs", "ttyd", "ffmpeg", "python3", "git"];
const TUI_CAPTURE_DEPS_INSTALL_HINT: &str = "cargo xtask doctor deps install --only vhs";

pub(crate) fn run_tui_vhs_demo(
    root: &Path,
    artifact_root: &Path,
    log: Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    let missing = missing_commands(TUI_CAPTURE_DEPS);
    if !missing.is_empty() {
        return failed_tui_capture(
            log,
            &[
                format!("missing VHS capture dependencies: {}", missing.join(" ")),
                format!("run: {TUI_CAPTURE_DEPS_INSTALL_HINT}"),
            ],
        );
    }

    let assets = root.join("xtask").join("fixtures").join("tui-capture");
    let fixture_cwd = assets.join("fixtures").join("cwd");
    let fixture_home = assets.join("fixtures").join("home");
    let mock_provider = assets.join("mock_provider.py");
    let tape_template = assets.join("pevo-tui-demo.tape.tpl");
    ensure_file(&mock_provider)?;
    ensure_file(&tape_template)?;
    ensure_dir(&fixture_cwd)?;
    ensure_dir(&fixture_home)?;

    let out_dir = tui_capture_artifact_dir(artifact_root);
    if out_dir.exists() {
        fs::remove_dir_all(&out_dir)
            .with_context(|| format!("remove stale TUI artifact dir {}", out_dir.display()))?;
    }
    fs::create_dir_all(&out_dir)
        .with_context(|| format!("create TUI artifact dir {}", out_dir.display()))?;

    let workdir_path = artifact_root.join("visual").join("tui-work-cwd");
    if workdir_path.exists() {
        fs::remove_dir_all(&workdir_path)
            .with_context(|| format!("remove stale TUI workdir {}", workdir_path.display()))?;
    }
    fs::create_dir_all(&workdir_path)
        .with_context(|| format!("create TUI workdir {}", workdir_path.display()))?;
    let _workdir = TempWorkDir::new(workdir_path.clone());

    let home = out_dir.join("home");
    fs::create_dir_all(&home).with_context(|| format!("create {}", home.display()))?;
    copy_dir_contents(&fixture_cwd, &workdir_path)?;

    let pevo_bin = pevo_bin_path(root);
    let mut mirrored_diagnostics = 0;
    if env::var_os("PEVO_BIN").is_none() {
        let mut cargo = ProcessCommand::new("cargo");
        cargo
            .args(["build", "-p", "psychevo-cli", "--quiet"])
            .current_dir(root)
            .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
        let outcome = run_logged_process("build psychevo-cli", &mut cargo, Arc::clone(&log))?;
        mirrored_diagnostics += outcome.mirrored_diagnostics;
        if !outcome.passed {
            return Ok(ProcessOutcome {
                passed: false,
                exit_code: outcome.exit_code,
                mirrored_diagnostics,
            });
        }
    }
    if !pevo_bin.is_file() {
        return failed_tui_capture(
            log,
            &[format!("pevo binary is missing: {}", pevo_bin.display())],
        );
    }

    let outcome = prepare_fixture_workdir(root, artifact_root, &workdir_path, &log)?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
        });
    }

    let port_file = out_dir.join("mock-provider.port");
    let request_log = out_dir.join("mock-provider-requests.ndjson");
    let mut provider_command = ProcessCommand::new("python3");
    provider_command
        .arg("-u")
        .arg(&mock_provider)
        .arg(&port_file)
        .arg(&request_log);
    let mut mock_provider =
        LoggedChild::spawn("TUI mock provider", provider_command, Arc::clone(&log))?;
    wait_for_file(&port_file, 100, Duration::from_millis(50))
        .with_context(|| "mock provider did not start")?;
    let port = fs::read_to_string(&port_file)
        .with_context(|| format!("read {}", port_file.display()))?
        .trim()
        .to_string();

    let mut init = ProcessCommand::new(&pevo_bin);
    init.arg("init")
        .current_dir(root)
        .env("PSYCHEVO_HOME", &home)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process("pevo init for TUI capture", &mut init, Arc::clone(&log))?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if !outcome.passed {
        mirrored_diagnostics += mock_provider.stop()?.mirrored_lines;
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
        });
    }
    copy_dir_contents(&fixture_home, &home)?;
    let config_path = home.join("config.toml");
    write_tui_capture_config(&config_path, &port)?;

    let db_path = home.join("state.db");
    let pevo_bin_arg = path_arg(&pevo_bin);
    let workdir_arg = path_arg(&workdir_path);
    let pevo_cmd = shell_quote_args(&[
        "env",
        "-u",
        "NO_COLOR",
        "TERM=xterm-256color",
        "COLORTERM=truecolor",
        "CLICOLOR_FORCE=1",
        &pevo_bin_arg,
        "tui",
        "--dir",
        &workdir_arg,
        "-m",
        "mock/mock-model",
        "--variant",
        "high",
        "--debug",
    ]);
    let tape = out_dir.join("pevo-tui-demo.tape");
    render_tui_capture_tape_file(
        &tape_template,
        &tape,
        &home,
        &db_path,
        &config_path,
        &pevo_cmd,
    )?;

    let mut vhs = ProcessCommand::new("vhs");
    vhs.arg(&tape)
        .current_dir(&out_dir)
        .env("PATH", path_with_capture_prefixes(root, &pevo_bin)?)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process("vhs TUI capture", &mut vhs, Arc::clone(&log))?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    mirrored_diagnostics += mock_provider.stop()?.mirrored_lines;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
        });
    }

    let missing = missing_tui_capture_screenshots(&out_dir);
    if !missing.is_empty() {
        return failed_tui_capture(
            log,
            &[format!(
                "VHS did not write expected screenshot(s): {}",
                missing.join(" ")
            )],
        );
    }
    write_log_line(
        &log,
        &format!("wrote TUI capture artifacts: {}", out_dir.display()),
    )?;

    Ok(ProcessOutcome {
        passed: true,
        exit_code: Some(0),
        mirrored_diagnostics,
    })
}

fn prepare_fixture_workdir(
    root: &Path,
    artifact_root: &Path,
    workdir_path: &Path,
    log: &Arc<Mutex<fs::File>>,
) -> Result<ProcessOutcome> {
    let mut mirrored_diagnostics = 0;
    let mut git_init = ProcessCommand::new("git");
    git_init
        .arg("-C")
        .arg(workdir_path)
        .args(["init", "-b", "main"])
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process(
        "prepare TUI fixture: git init",
        &mut git_init,
        Arc::clone(log),
    )?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
        });
    }
    fs::write(
        workdir_path.join(".git").join("info").join("exclude"),
        "fixture.txt\n.psychevo/\n",
    )
    .with_context(|| "write TUI fixture git exclude")?;
    write_inline_diff_fixture(workdir_path)?;

    let mut git_add = ProcessCommand::new("git");
    git_add
        .arg("-C")
        .arg(workdir_path)
        .args(["add", "inline-diff-fixture.txt"])
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process(
        "prepare TUI fixture: git add",
        &mut git_add,
        Arc::clone(log),
    )?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
        });
    }

    let mut commit = ProcessCommand::new("git");
    commit
        .arg("-C")
        .arg(workdir_path)
        .args(["-c", "user.name=Psychevo VHS"])
        .args(["-c", "user.email=psychevo-vhs@example.invalid"])
        .args(["commit", "-m", "add inline diff fixture"])
        .current_dir(root)
        .env("PSYCHEVO_CI_ARTIFACT_ROOT", artifact_root);
    let outcome = run_logged_process(
        "prepare TUI fixture: git commit",
        &mut commit,
        Arc::clone(log),
    )?;
    mirrored_diagnostics += outcome.mirrored_diagnostics;
    if !outcome.passed {
        return Ok(ProcessOutcome {
            passed: false,
            exit_code: outcome.exit_code,
            mirrored_diagnostics,
        });
    }
    write_diff_demo_fixture(workdir_path)?;
    Ok(ProcessOutcome {
        passed: true,
        exit_code: Some(0),
        mirrored_diagnostics,
    })
}

fn failed_tui_capture(log: Arc<Mutex<fs::File>>, lines: &[String]) -> Result<ProcessOutcome> {
    for line in lines {
        write_mirrored_line(&log, line)?;
    }
    Ok(ProcessOutcome {
        passed: false,
        exit_code: Some(1),
        mirrored_diagnostics: lines.len(),
    })
}

fn missing_commands(commands: &[&str]) -> Vec<String> {
    commands
        .iter()
        .copied()
        .filter(|command| !command_exists(command))
        .map(str::to_string)
        .collect()
}

fn ensure_file(path: &Path) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        bail!("missing required file: {}", path.display())
    }
}

fn ensure_dir(path: &Path) -> Result<()> {
    if path.is_dir() {
        Ok(())
    } else {
        bail!("missing required directory: {}", path.display())
    }
}

pub(crate) fn tui_capture_artifact_dir(artifact_root: &Path) -> PathBuf {
    artifact_root.join("visual").join("tui")
}

fn pevo_bin_path(root: &Path) -> PathBuf {
    env::var_os("PEVO_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("target").join("debug").join("pevo"))
}

fn copy_dir_contents(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target).with_context(|| format!("create {}", target.display()))?;
    for entry in fs::read_dir(source).with_context(|| format!("read {}", source.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", source.display()))?;
        let source_path = entry.path();
        let target_path = target.join(entry.file_name());
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type for {}", source_path.display()))?;
        if file_type.is_dir() {
            copy_dir_contents(&source_path, &target_path)?;
        } else if file_type.is_file() {
            fs::copy(&source_path, &target_path).with_context(|| {
                format!(
                    "copy {} to {}",
                    source_path.display(),
                    target_path.display()
                )
            })?;
        }
    }
    Ok(())
}

fn write_inline_diff_fixture(workdir: &Path) -> Result<()> {
    fs::write(
        workdir.join("inline-diff-fixture.txt"),
        "line 01: context before\nline 02: limit = 1000\nline 03: context after\n",
    )
    .with_context(|| "write inline diff fixture")
}

fn write_diff_demo_fixture(workdir: &Path) -> Result<()> {
    fs::write(
        workdir.join("diff-demo.rs"),
        "pub fn diff_overlay_fixture() -> &'static str {\n    \"VHS diff overlay\"\n}\n",
    )
    .with_context(|| "write diff demo fixture")
}

fn path_arg(path: &Path) -> String {
    path.display().to_string()
}

fn wait_for_file(path: &Path, tries: usize, interval: Duration) -> Result<()> {
    for _ in 0..tries {
        if path.is_file()
            && fs::metadata(path)
                .map(|metadata| metadata.len() > 0)
                .unwrap_or(false)
        {
            return Ok(());
        }
        thread::sleep(interval);
    }
    bail!("timed out waiting for {}", path.display())
}

fn write_tui_capture_config(path: &Path, port: &str) -> Result<()> {
    fs::write(
        path,
        format!(
            r#"model = "mock/mock-model"

[provider.mock]
api = "http://127.0.0.1:{port}/v1"
no_auth = true

[provider.mock.models.mock-model]
reasoning_effort = "high"

[provider.mock.models.mock-model.limit]
context = 64000

[provider.mock.models.other-model]
reasoning_effort = "medium"

[provider.mock.models.other-model.limit]
context = 32000
"#
        ),
    )
    .with_context(|| format!("write {}", path.display()))
}

fn render_tui_capture_tape_file(
    template_path: &Path,
    output_path: &Path,
    psychevo_home: &Path,
    psychevo_db: &Path,
    psychevo_config: &Path,
    pevo_cmd: &str,
) -> Result<()> {
    let template = fs::read_to_string(template_path)
        .with_context(|| format!("read {}", template_path.display()))?;
    let psychevo_home = path_arg(psychevo_home);
    let psychevo_db = path_arg(psychevo_db);
    let psychevo_config = path_arg(psychevo_config);
    let rendered = render_tui_capture_tape(
        &template,
        &[
            ("PSYCHEVO_HOME", psychevo_home.as_str()),
            ("PSYCHEVO_DB", psychevo_db.as_str()),
            ("PSYCHEVO_CONFIG", psychevo_config.as_str()),
            ("PEVO_CMD", pevo_cmd),
        ],
    )?;
    fs::write(output_path, rendered).with_context(|| format!("write {}", output_path.display()))
}

fn render_tui_capture_tape(template: &str, replacements: &[(&str, &str)]) -> Result<String> {
    let mut rendered = template.to_string();
    for (placeholder, value) in replacements {
        let placeholder = format!("{{{{{placeholder}}}}}");
        rendered = rendered.replace(&placeholder, &serde_json::to_string(value)?);
    }
    let unresolved = unresolved_tape_placeholders(&rendered);
    if !unresolved.is_empty() {
        bail!("unresolved tape placeholder(s): {}", unresolved.join(", "));
    }
    Ok(rendered)
}

fn unresolved_tape_placeholders(text: &str) -> Vec<String> {
    let mut placeholders = HashSet::new();
    let mut offset = 0;
    while let Some(start_relative) = text[offset..].find("{{") {
        let start = offset + start_relative;
        let Some(end_relative) = text[start + 2..].find("}}") else {
            break;
        };
        let end = start + 2 + end_relative + 2;
        let placeholder = &text[start..end];
        let name = &placeholder[2..placeholder.len() - 2];
        if name
            .chars()
            .all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
        {
            placeholders.insert(placeholder.to_string());
        }
        offset = end;
    }
    let mut placeholders: Vec<_> = placeholders.into_iter().collect();
    placeholders.sort();
    placeholders
}

fn shell_quote_args(args: &[&str]) -> String {
    args.iter()
        .map(|arg| shell_quote_arg(arg))
        .collect::<Vec<_>>()
        .join(" ")
}

fn shell_quote_arg(arg: &str) -> String {
    if arg.is_empty() {
        return "''".to_string();
    }
    if arg
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':' | '='))
    {
        return arg.to_string();
    }
    format!("'{}'", arg.replace('\'', r#"'\''"#))
}

fn path_with_capture_prefixes(root: &Path, pevo_bin: &Path) -> Result<OsString> {
    let mut paths = Vec::new();
    if let Some(parent) = pevo_bin.parent() {
        paths.push(parent.to_path_buf());
    }
    paths.push(root.join("target").join("debug"));
    if let Some(current) = env::var_os("PATH") {
        paths.extend(env::split_paths(&current));
    }
    env::join_paths(paths).context("join PATH for TUI capture")
}

fn missing_tui_capture_screenshots(out_dir: &Path) -> Vec<String> {
    TUI_CAPTURE_SCREENSHOTS
        .iter()
        .copied()
        .filter(|screenshot| {
            let path = out_dir.join(screenshot);
            !path.is_file()
                || fs::metadata(&path)
                    .map(|metadata| metadata.len() == 0)
                    .unwrap_or(true)
        })
        .map(str::to_string)
        .collect()
}

#[cfg(test)]
fn screenshot_names_in_tape_template(template: &str) -> Vec<String> {
    template
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let rest = line.strip_prefix("Screenshot \"")?;
            let end = rest.find('"')?;
            Some(rest[..end].to_string())
        })
        .collect()
}

struct TempWorkDir {
    path: PathBuf,
}

impl TempWorkDir {
    fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl Drop for TempWorkDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tape_rendering_replaces_all_placeholders() {
        let rendered = render_tui_capture_tape(
            "Env PSYCHEVO_HOME {{PSYCHEVO_HOME}}\nType {{PEVO_CMD}}\n",
            &[("PSYCHEVO_HOME", "/tmp/home"), ("PEVO_CMD", "pevo tui")],
        )
        .expect("render tape");
        assert_eq!(
            rendered,
            "Env PSYCHEVO_HOME \"/tmp/home\"\nType \"pevo tui\"\n"
        );
    }

    #[test]
    fn tape_rendering_rejects_unresolved_placeholders() {
        let err = render_tui_capture_tape("Env PSYCHEVO_HOME {{PSYCHEVO_HOME}}\n", &[])
            .expect_err("unresolved placeholder");
        assert!(err.to_string().contains("{{PSYCHEVO_HOME}}"));
    }

    #[test]
    fn expected_screenshot_inventory_is_stable_and_matches_template_shape() {
        let mut seen = HashSet::new();
        for screenshot in TUI_CAPTURE_SCREENSHOTS {
            assert!(
                seen.insert(*screenshot),
                "duplicate screenshot {screenshot}"
            );
            assert!(screenshot.ends_with(".png"));
        }
        let template = include_str!("../../fixtures/tui-capture/pevo-tui-demo.tape.tpl");
        assert_eq!(
            screenshot_names_in_tape_template(template),
            TUI_CAPTURE_SCREENSHOTS
        );
    }

    #[test]
    fn artifact_path_uses_ci_root_visual_tui_subdir() {
        assert_eq!(
            tui_capture_artifact_dir(Path::new("/tmp/run")),
            PathBuf::from("/tmp/run/visual/tui")
        );
    }

    #[test]
    fn missing_vhs_dependency_hint_points_to_xtask_doctor_deps() {
        assert_eq!(
            TUI_CAPTURE_DEPS_INSTALL_HINT,
            "cargo xtask doctor deps install --only vhs"
        );
    }
}
