#[allow(unused_imports)]
use super::*;

pub(crate) fn resolved_artifact_includes(
    project: &EvalProject,
    cli_includes: &[String],
) -> BTreeSet<String> {
    project
        .artifacts
        .include
        .iter()
        .chain(cli_includes.iter())
        .flat_map(|value| value.split(','))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
        .collect()
}

pub(crate) fn execute_cell(
    project: &EvalProject,
    case: CasePlan,
    cell_root: &Path,
    cell_key: &str,
    fingerprint: &str,
    artifact_includes: &BTreeSet<String>,
) -> Result<CellRun> {
    let parent = cell_root
        .parent()
        .with_context(|| format!("cell root has no parent: {}", cell_root.display()))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temp = parent.join(format!(".tmp-{}-{}", cell_key, Uuid::now_v7()));
    if temp.exists() {
        fs::remove_dir_all(&temp)
            .with_context(|| format!("failed to remove {}", temp.display()))?;
    }
    fs::create_dir_all(&temp).with_context(|| format!("failed to create {}", temp.display()))?;
    let started_at_ms = now_ms();
    let result = run_case(&temp, case, artifact_includes)?;
    let finished_at_ms = now_ms();
    let cell = CellRun {
        schema_version: ARTIFACT_SCHEMA_VERSION,
        benchmark: project.benchmark_id.clone(),
        benchmark_slug: project.slug(),
        cell_key: cell_key.to_string(),
        fingerprint: fingerprint.to_string(),
        cell_root: cell_root.to_path_buf(),
        started_at_ms,
        finished_at_ms,
        case: result,
    };
    write_json_pretty(&temp.join("run.json"), &cell)?;
    if cell_root.exists() {
        fs::remove_dir_all(cell_root)
            .with_context(|| format!("failed to replace {}", cell_root.display()))?;
    }
    fs::rename(&temp, cell_root).with_context(|| {
        format!(
            "failed to move {} to {}",
            temp.display(),
            cell_root.display()
        )
    })?;
    read_cell_run(cell_root)
}

pub(crate) fn cell_key(fingerprint: &str) -> String {
    fingerprint.chars().take(16).collect::<String>()
}

pub(crate) fn cell_fingerprint(project: &EvalProject, case: &CasePlan) -> Result<String> {
    let workspace_source = resolve_relative(&case.task.dir, &case.task.workspace.source);
    let payload = json!({
        "artifact_schema_version": ARTIFACT_SCHEMA_VERSION,
        "runner": "psychevo-eval-cell-v8",
        "benchmark": {
            "id": &project.benchmark_id,
            "name": &project.benchmark_name,
            "slug": project.slug(),
        },
        "task_set": {
            "id": &case.task_set.id,
        },
        "task": {
            "id": &case.task.id,
            "kind": &case.task.kind,
            "source_kind": case.task.source_kind,
            "source_id": &case.task.source_id,
            "native_id": &case.task.native_id,
            "definition": serde_json::to_value(&case.task)?,
            "prompt": task_prompt(&case.task)?,
            "workspace": workspace_tree_hash(&workspace_source)?,
        },
        "agent": {
            "id": &case.agent.id,
            "kind": case.agent.kind,
            "model": agent_model(&case.agent),
            "fake": &case.agent.fake,
            "command": &case.agent.command,
            "acp": &case.agent.acp,
            "psychevo": &case.agent.psychevo,
            "opencode": &case.agent.opencode,
            "hermes": &case.agent.hermes,
        },
        "factors": {},
    });
    Ok(stable_hash_hex(&serde_json::to_string(&payload)?))
}

pub(crate) fn workspace_tree_hash(root: &Path) -> Result<String> {
    if !root.exists() {
        bail!("workspace source does not exist: {}", root.display());
    }
    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hash = Fnv64::new();
    for (relative, path) in files {
        hash.add(relative.to_string_lossy().as_bytes());
        hash.add(&[0]);
        let bytes =
            fs::read(&path).with_context(|| format!("failed to read {}", path.display()))?;
        hash.add(&bytes);
        hash.add(&[0]);
    }
    Ok(format!("{:016x}", hash.finish()))
}

fn collect_files(root: &Path, dir: &Path, files: &mut Vec<(PathBuf, PathBuf)>) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_files(root, &path, files)?;
        } else if file_type.is_file() {
            let relative = path.strip_prefix(root).unwrap_or(&path).to_path_buf();
            files.push((relative, path));
        }
    }
    Ok(())
}

pub(crate) fn stable_hash_hex(value: &str) -> String {
    stable_hash_bytes(value.as_bytes())
}

pub(crate) fn stable_hash_bytes(value: &[u8]) -> String {
    let mut hash = Fnv64::new();
    hash.add(value);
    format!("{:016x}", hash.finish())
}

struct Fnv64(u64);

impl Fnv64 {
    fn new() -> Self {
        Self(0xcbf29ce484222325)
    }

    fn add(&mut self, bytes: &[u8]) {
        for byte in bytes {
            self.0 ^= u64::from(*byte);
            self.0 = self.0.wrapping_mul(0x100000001b3);
        }
    }

    fn finish(self) -> u64 {
        self.0
    }
}
