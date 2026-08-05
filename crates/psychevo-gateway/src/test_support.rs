use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use psychevo::host_paths::{ExecutableResolveOptions, HostPlatform, resolve_executable_path};

pub(crate) const ONE_PIXEL_PNG_BASE64: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mOsvmfPfwAH5QMm7n0ViwAAAABJRU5ErkJggg==";

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
    let program = resolve_executable_path(
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
    AcpFixture { program, script }
}

pub(crate) fn toml_string(value: impl AsRef<str>) -> String {
    serde_json::to_string(value.as_ref()).expect("quote test TOML string")
}

pub(crate) fn toml_path(path: &Path) -> String {
    toml_string(path.to_string_lossy())
}
