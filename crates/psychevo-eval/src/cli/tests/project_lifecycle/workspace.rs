use super::support::*;
#[allow(unused_imports)]
use super::*;
use pretty_assertions::assert_eq;

#[test]
pub(crate) fn init_creates_v2_workspace_without_cache_or_dashboard() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("evals");
    let initialized = init_eval_store(InitStoreRequest {
        root: Some(root.clone()),
        make_default: false,
        force: false,
    })
    .expect("init");

    assert_eq!(initialized.schema_version, WORKSPACE_SCHEMA_VERSION);
    assert_eq!(initialized.root, absolute_path(&root));
    assert!(initialized.root.join("peval.toml").is_file());
    assert!(initialized.root.join("runs").is_dir());
    assert!(initialized.root.join("datasets").is_dir());
    assert!(initialized.root.join("scripts").is_dir());
    assert!(
        initialized
            .root
            .join("pidx-psychevo-acp.eval.toml")
            .is_file()
    );
    assert!(!initialized.root.join("eval.toml").exists());
    assert!(
        !initialized
            .root
            .join("pidx-fake-patch-add.eval.toml")
            .exists()
    );
    assert!(
        !initialized
            .root
            .join("pidx-psychevo-patch-add.eval.toml")
            .exists()
    );
    assert!(!initialized.root.join(".cache").exists());
    assert!(!initialized.root.join("dashboard.html").exists());

    let workspace = read_workspace_config(&initialized.root).expect("workspace config");
    assert_eq!(workspace.schema_version, WORKSPACE_SCHEMA_VERSION);
    assert!(workspace.agents.is_empty());
    assert!(workspace.benchmarks.is_empty());
}

#[test]
pub(crate) fn discover_manifest_uses_unique_eval_template_config() {
    let temp = tempfile::tempdir().expect("temp");
    let root = init_workspace(temp.path().join("evals"));

    let manifest = discover_manifest(&root).expect("discover unique eval template");
    assert_eq!(manifest, root.join("pidx-psychevo-acp.eval.toml"));
}

#[test]
pub(crate) fn discover_manifest_rejects_ambiguous_eval_templates() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("evals");
    fs::create_dir_all(&root).expect("root");
    fs::write(root.join("a.eval.toml"), "").expect("a eval");
    fs::write(root.join("b.eval.toml"), "").expect("b eval");

    let err = discover_manifest(&root).expect_err("ambiguous eval templates should fail");
    let message = format!("{err:#}");
    assert!(message.contains("multiple eval config TOML files found"));
    assert!(message.contains("--config <path>"));
    assert!(message.contains("a.eval.toml"));
    assert!(message.contains("b.eval.toml"));
}

#[test]
pub(crate) fn discover_manifest_prefers_eval_toml_over_eval_templates() {
    let temp = tempfile::tempdir().expect("temp");
    let fixture = create_local_coding_eval(&temp.path().join("test-coding"));
    fs::write(fixture.join("a.eval.toml"), "not toml").expect("a eval");
    fs::write(fixture.join("b.eval.toml"), "not toml").expect("b eval");

    let manifest = discover_manifest(&fixture).expect("discover eval.toml first");
    assert_eq!(manifest, fixture.join("eval.toml"));

    let project = EvalProject::load(&fixture).expect("load eval.toml despite sibling templates");
    assert_eq!(project.id, "test-coding-eval");
}
