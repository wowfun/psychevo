use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

fn peval() -> &'static str {
    env!("CARGO_BIN_EXE_peval")
}

fn default_template() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("templates")
        .join("pidx-psychevo-acp.eval.toml")
}

fn write_pidx_eval_config(root: &std::path::Path) -> PathBuf {
    let config = root.join("pidx.eval.toml");
    std::fs::write(
        &config,
        r#"schema_version = 5
id = "pidx-coding-cli"
name = "pidx coding CLI"

[benchmark]
id = "pidx-coding"

[select]
agents = ["psychevo", "opencode", "hermes"]
sets = ["pidx"]

[[agents]]
id = "psychevo"
kind = "psychevo-acp"

[[agents]]
id = "opencode"
kind = "opencode-acp"

[[agents]]
id = "hermes"
kind = "hermes-acp"
"#,
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
fn default_template_check_is_public_cli_contract() {
    let temp = tempfile::tempdir().expect("temp");
    let output = Command::new(peval())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PSYCHEVO_HOME", temp.path().join("psychevo-home"))
        .arg("check")
        .arg("--config")
        .arg(default_template())
        .arg("--json")
        .output()
        .expect("peval check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("json stdout");
    assert_eq!(json["benchmark"], "pidx-coding");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["cases"], 1);
}

#[test]
fn check_discovers_unique_eval_template_from_workspace_root() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("evals");
    let init = Command::new(peval())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PSYCHEVO_HOME", temp.path().join("psychevo-home"))
        .arg("init")
        .arg("--root")
        .arg(&root)
        .arg("--json")
        .output()
        .expect("peval init");
    assert!(
        init.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&init.stderr)
    );

    let output = Command::new(peval())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PSYCHEVO_HOME", temp.path().join("psychevo-home"))
        .current_dir(&root)
        .arg("check")
        .arg("--root")
        .arg(&root)
        .arg("--json")
        .output()
        .expect("peval check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("json stdout");
    assert_eq!(json["benchmark"], "pidx-coding");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["cases"], 1);
}

#[test]
fn check_reports_ambiguous_eval_templates_without_eval_toml() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("evals");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(root.join("a.eval.toml"), "").expect("a eval");
    std::fs::write(root.join("b.eval.toml"), "").expect("b eval");

    let output = Command::new(peval())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PSYCHEVO_HOME", temp.path().join("psychevo-home"))
        .current_dir(&root)
        .arg("check")
        .arg("--root")
        .arg(&root)
        .arg("--json")
        .output()
        .expect("peval check");

    assert!(
        !output.status.success(),
        "stdout: {}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("multiple eval config TOML files found"));
    assert!(stderr.contains("--config <path>"));
    assert!(stderr.contains("a.eval.toml"));
    assert!(stderr.contains("b.eval.toml"));
}

#[test]
fn check_prefers_eval_toml_over_eval_templates() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("evals");
    std::fs::create_dir_all(&root).expect("root");
    std::fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "explicit-eval"
name = "explicit eval"

[benchmark]
id = "pidx-coding"

[select]
agents = ["fake-pass"]
sets = ["pidx/smoke"]
tasks = ["pidx/patch-add"]

[[agents]]
id = "fake-pass"
kind = "fake"
fake = { behavior = "pass" }
"#,
    )
    .expect("eval");
    std::fs::write(root.join("a.eval.toml"), "").expect("a eval");
    std::fs::write(root.join("b.eval.toml"), "").expect("b eval");

    let output = Command::new(peval())
        .env_clear()
        .env("HOME", temp.path().join("home"))
        .env("PSYCHEVO_HOME", temp.path().join("psychevo-home"))
        .current_dir(&root)
        .arg("check")
        .arg("--root")
        .arg(&root)
        .arg("--json")
        .output()
        .expect("peval check");

    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let json: Value = serde_json::from_slice(&output.stdout).expect("json stdout");
    assert_eq!(json["eval"], "explicit eval");
    assert_eq!(json["status"], "ok");
    assert_eq!(json["cases"], 1);
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
