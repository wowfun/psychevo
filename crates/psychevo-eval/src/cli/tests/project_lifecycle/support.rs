#[allow(unused_imports)]
use super::*;

pub(crate) fn init_workspace(root: PathBuf) -> PathBuf {
    init_eval_store(InitStoreRequest {
        root: Some(root.clone()),
        make_default: false,
        force: false,
    })
    .expect("init")
    .root
}

pub(crate) fn create_local_coding_eval(root: &Path) -> PathBuf {
    fs::create_dir_all(root).expect("project root");
    fs::write(
        root.join("benchmark.toml"),
        r#"schema_version = 5
id = "test-coding"
name = "test-coding"

[[sources.peval_agent]]
id = "local"
path = "tasks"
verifier_timeout_seconds = 600

[[sources.peval_agent.sets]]
id = "rust-swe"
include = ["rust-swe-add"]
"#,
    )
    .expect("benchmark");
    fs::write(
        root.join("eval.toml"),
        r#"schema_version = 5
id = "test-coding-eval"
name = "test-coding eval"

[benchmark]
path = "benchmark.toml"

[select]
agents = ["fake-pass", "fake-fail"]
sets = ["local/rust-swe"]

[[agents]]
id = "fake-pass"
kind = "command"
command = { command = "sh", args = ["-c", "printf fixed > status.txt"] }

[[agents]]
id = "fake-fail"
kind = "command"
command = { command = "sh", args = ["-c", ":"] }
"#,
    )
    .expect("eval");
    write_local_task(root, "rust-swe-add", "swe-style");
    root.to_path_buf()
}

pub(crate) fn write_local_task(root: &Path, id: &str, kind: &str) {
    let dir = root.join("tasks").join(id);
    fs::create_dir_all(dir.join("environment")).expect("environment");
    fs::create_dir_all(dir.join("tests")).expect("tests");
    fs::write(dir.join("environment/status.txt"), "pending").expect("status");
    fs::write(
        dir.join("task.toml"),
        format!("name = \"complete {id}\"\nkind = \"{kind}\"\n"),
    )
    .expect("task toml");
    fs::write(dir.join("instruction.md"), format!("complete {id}\n")).expect("instruction");
    fs::write(
        dir.join("tests/test.sh"),
        format!(
            r#"set -e
test "$PEVAL_TASK_ID" = "local/{id}"
test "$PEVAL_NATIVE_TASK_ID" = "{id}"
test "$PEVAL_SOURCE_ID" = "local"
test -d "$PEVAL_WORKSPACE"
test -d "$PEVAL_TASK_DIR"
test -d "$PEVAL_LOGS"
test "$(cat status.txt 2>/dev/null || true)" = fixed
mkdir -p "$PEVAL_LOGS/verifier"
printf '{{"message":"env checked","score":1.0}}' > "$PEVAL_LOGS/verifier/result.json"
"#
        ),
    )
    .expect("verifier");
}
