use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn peval() -> &'static str {
    env!("CARGO_BIN_EXE_peval")
}

fn pidx_benchmark() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benchmarks/pidx-coding/benchmark.toml")
}

fn write_pidx_eval_config(root: &std::path::Path) -> PathBuf {
    let config = root.join("pidx.eval.toml");
    std::fs::write(
        &config,
        format!(
            r#"schema_version = 5
id = "pidx-coding-cli"
name = "pidx coding CLI"

[benchmark]
path = "{}"

[select]
agents = ["psychevo", "opencode", "hermes"]
sets = ["pidx"]

[[agents]]
id = "psychevo"
kind = "psychevo"

[[agents]]
id = "opencode"
kind = "opencode"

[[agents]]
id = "hermes"
kind = "hermes"
"#,
            pidx_benchmark().display()
        ),
    )
    .expect("write eval config");
    config
}

#[test]
fn pidx_coding_check_is_public_cli_contract() {
    let temp = tempfile::tempdir().expect("temp");
    let config = write_pidx_eval_config(temp.path());
    let output = Command::new(peval())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PSYCHEVO_HOME", temp.path().join("psychevo-home"))
        .arg("check")
        .arg("--config")
        .arg(config)
        .arg("--json")
        .output()
        .expect("peval check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "",
        "check should not emit stderr on success"
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("json stdout");
    assert_eq!(json["schema_version"], 5);
    assert_eq!(json["benchmark"], "pidx-coding");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["cases"], 9);
}

#[test]
fn pidx_coding_check_filters_single_adapter_matrix() {
    let temp = tempfile::tempdir().expect("temp");
    let config = write_pidx_eval_config(temp.path());
    let output = Command::new(peval())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PSYCHEVO_HOME", temp.path().join("psychevo-home"))
        .arg("check")
        .arg("--config")
        .arg(config)
        .arg("--task-set")
        .arg("pidx")
        .arg("--agent")
        .arg("psychevo")
        .arg("--json")
        .output()
        .expect("peval check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("json stdout");
    assert_eq!(json["schema_version"], 5);
    assert_eq!(json["benchmark"], "pidx-coding");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["cases"], 3);
}
