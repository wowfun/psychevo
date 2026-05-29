#[allow(unused_imports)]
use super::*;

pub(crate) fn apply_fake_pass_fixes(task: &TaskManifest, workspace: &Path) -> Result<Vec<PathBuf>> {
    let mut changed = Vec::new();
    for check in &task.test_spec.checks {
        if let LocalCodingCheck::ExactFile { path, expected } = check {
            let target = resolve_relative(workspace, path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("failed to create {}", parent.display()))?;
            }
            fs::write(&target, expected)
                .with_context(|| format!("failed to write {}", target.display()))?;
            changed.push(path.clone());
        }
    }
    Ok(changed)
}
