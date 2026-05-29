use super::support::*;
#[allow(unused_imports)]
use super::*;
use pretty_assertions::assert_eq;

#[test]
pub(crate) fn official_sources_require_compatible_bridges() {
    let temp = tempfile::tempdir().expect("temp");
    let root = temp.path().join("tau2-eval");
    fs::create_dir_all(&root).expect("project root");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 5
id = "tau2-declaration"
name = "tau2-declaration"

[[sources.tau2]]
id = "tau"
root = "."
domain = "airline"
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "tau2-declaration-eval"
name = "tau2 declaration eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass"]
sets = ["tau"]

[[agents]]
id = "fake-pass"
kind = "command"

[agents.command]
command = "sh"
args = ["-c", ":"]
"#,
    )
    .expect("eval");
    let project = EvalProject::load(root.join("eval.toml")).expect("external project");
    let err = check_project(&project, None, None, None).expect_err("official bridge required");
    assert!(
        format!("{err:#}").contains("incompatible_source_agent"),
        "{err:#}"
    );

    let denied = EvalService::new(ServiceContext {
        cwd: root.clone(),
        env: BTreeMap::new(),
        psychevo_home: Some(temp.path().join("psychevo-home")),
        root_override: Some(init_workspace(temp.path().join("evals"))),
        capabilities: ServiceCapabilities::all(),
    })
    .run(RunRequest {
        config: Some(root.join("eval.toml")),
        benchmark: None,
        task_set: None,
        task: None,
        agent: None,
        overwrite: false,
        store_root: None,
        output_root: None,
        include_artifacts: Vec::new(),
    })
    .expect_err("official source run should fail");
    assert_eq!(denied.code, "incompatible_source_agent");
}
