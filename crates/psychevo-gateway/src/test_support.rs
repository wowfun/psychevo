use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[cfg(windows)]
use std::sync::atomic::{AtomicU64, Ordering};

use psychevo::host_paths::{ExecutableResolveOptions, HostPlatform, resolve_executable_path};

pub(crate) const ONE_PIXEL_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mOsvmfPfwAH5QMm7n0ViwAAAABJRU5ErkJggg==";

#[cfg(windows)]
static NEXT_ACP_LAUNCHER_ID: AtomicU64 = AtomicU64::new(1);

pub(crate) struct AcpFixture {
    pub(crate) program: PathBuf,
    pub(crate) script: PathBuf,
}

pub(crate) fn acp_fixture(cwd: &Path, name: &str) -> AcpFixture {
    let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
    #[cfg(windows)]
    let (command, extension) = ("node", "js");
    #[cfg(unix)]
    let (command, extension) = ("python3", "py");
    let resolved_program = resolve_executable_path(
        command,
        cwd,
        &ExecutableResolveOptions {
            platform: HostPlatform::current(),
            env: &host_env,
        },
    )
    .unwrap_or_else(|| panic!("resolve {command} for ACP test fixture"));
    let script = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.{extension}"));
    assert!(
        script.is_file(),
        "missing ACP test fixture {}",
        script.display()
    );
    #[cfg(windows)]
    {
        let system_root = host_env
            .iter()
            .find(|(key, _)| key.eq_ignore_ascii_case("SystemRoot"))
            .map(|(_, value)| value)
            .expect("SystemRoot for Windows ACP test fixture");
        let launcher = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/psychevo-test-fixtures")
            .join(std::process::id().to_string());
        std::fs::create_dir_all(&launcher).expect("Windows ACP fixture launcher directory");
        let program = launcher.join(format!(
            "node-{}.cmd",
            NEXT_ACP_LAUNCHER_ID.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::write(
            &program,
            format!(
                "@echo off\r\nset \"SystemRoot={system_root}\"\r\n\"{}\" %*\r\n",
                resolved_program.display()
            ),
        )
        .expect("Windows ACP fixture launcher script");
        AcpFixture { program, script }
    }
    #[cfg(unix)]
    {
        AcpFixture {
            program: resolved_program,
            script,
        }
    }
}

pub(crate) fn toml_string(value: impl AsRef<str>) -> String {
    serde_json::to_string(value.as_ref()).expect("quote test TOML string")
}

pub(crate) fn toml_path(path: &Path) -> String {
    toml_string(path.to_string_lossy())
}

pub(crate) fn native_process_fixture_env(
    env: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    #[cfg(windows)]
    let mut env = env;
    #[cfg(windows)]
    {
        let host_env = std::env::vars().collect::<BTreeMap<_, _>>();
        let git_bash = psychevo::host_paths::GitBashRuntime::discover(&host_env)
            .expect("Git Bash for native Windows process fixture");
        env.insert(
            psychevo::host_paths::PSYCHEVO_GIT_BASH_PATH.to_string(),
            git_bash.bash.to_string_lossy().into_owned(),
        );
        env.insert(
            "SystemRoot".to_string(),
            host_env
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case("SystemRoot"))
                .map(|(_, value)| value.clone())
                .expect("SystemRoot for native Windows process fixture"),
        );
    }
    env
}
