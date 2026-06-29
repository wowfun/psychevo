use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::artifacts::default_ci_root;

const CI_RUN_RETENTION: usize = 10;

pub(crate) fn warn_if_ci_retention_cleanup_fails(root: &Path, protected_run: &Path) {
    if let Err(error) = prune_default_ci_runs(root, protected_run) {
        eprintln!("warning: failed to prune CI artifact runs: {error:#}");
    }
}

fn prune_default_ci_runs(root: &Path, protected_run: &Path) -> Result<Vec<PathBuf>> {
    prune_ci_runs(
        &default_ci_root(root),
        CI_RUN_RETENTION,
        Some(protected_run),
    )
}

fn prune_ci_runs(
    ci_root: &Path,
    keep: usize,
    protected_run: Option<&Path>,
) -> Result<Vec<PathBuf>> {
    if !ci_root.is_dir() {
        return Ok(Vec::new());
    }

    let mut runs = numeric_ci_run_dirs(ci_root)?;
    runs.sort_by(|a, b| b.run_id.cmp(&a.run_id).then_with(|| b.path.cmp(&a.path)));

    let protected_run = protected_run.map(normalize_path_for_compare);
    let mut keep_paths: Vec<PathBuf> = runs
        .iter()
        .take(keep)
        .map(|run| normalize_path_for_compare(&run.path))
        .collect();
    if let Some(protected_run) = protected_run {
        let protected_exists = runs
            .iter()
            .any(|run| normalize_path_for_compare(&run.path) == protected_run);
        if protected_exists && !keep_paths.contains(&protected_run) && keep > 0 {
            if keep_paths.len() >= keep {
                keep_paths.pop();
            }
            keep_paths.push(protected_run);
        }
    }

    let keep_paths: HashSet<_> = keep_paths.into_iter().collect();
    let mut removed = Vec::new();
    for run in runs {
        if keep_paths.contains(&normalize_path_for_compare(&run.path)) {
            continue;
        }
        fs::remove_dir_all(&run.path)
            .with_context(|| format!("remove old CI artifact run {}", run.path.display()))?;
        removed.push(run.path);
    }
    Ok(removed)
}

#[derive(Debug)]
struct CiRunDir {
    path: PathBuf,
    run_id: u64,
}

fn numeric_ci_run_dirs(ci_root: &Path) -> Result<Vec<CiRunDir>> {
    let mut runs = Vec::new();
    for entry in fs::read_dir(ci_root).with_context(|| format!("read {}", ci_root.display()))? {
        let entry = entry.with_context(|| format!("read entry in {}", ci_root.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("read file type for {}", entry.path().display()))?;
        if !file_type.is_dir() {
            continue;
        }
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(run_id) = file_name.parse::<u64>() else {
            continue;
        };
        runs.push(CiRunDir {
            path: entry.path(),
            run_id,
        });
    }
    Ok(runs)
}

fn normalize_path_for_compare(path: &Path) -> PathBuf {
    path.components().collect()
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    #[test]
    fn ci_retention_keeps_recent_numeric_runs_and_ignores_other_entries() {
        let temp = unique_temp_dir("psychevo-xtask-ci-retention");
        let ci_root = temp.join("ci");
        fs::create_dir_all(&ci_root).expect("ci root");
        for run_id in 1..=12 {
            fs::create_dir_all(ci_root.join(format!("{run_id:04}"))).expect("run dir");
        }
        fs::create_dir_all(ci_root.join("notes")).expect("non-run dir");
        fs::write(ci_root.join("README"), "keep").expect("non-run file");

        let removed = prune_ci_runs(&ci_root, 10, None).expect("prune ci runs");
        let removed_names: Vec<_> = removed
            .iter()
            .map(|path| path.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(removed_names, vec!["0002", "0001"]);
        assert!(!ci_root.join("0001").exists());
        assert!(!ci_root.join("0002").exists());
        for run_id in 3..=12 {
            assert!(ci_root.join(format!("{run_id:04}")).is_dir());
        }
        assert!(ci_root.join("notes").is_dir());
        assert!(ci_root.join("README").is_file());

        let _ = fs::remove_dir_all(temp);
    }

    #[test]
    fn ci_retention_keeps_protected_run_even_when_it_is_old() {
        let temp = unique_temp_dir("psychevo-xtask-ci-retention-protected");
        let ci_root = temp.join("ci");
        fs::create_dir_all(&ci_root).expect("ci root");
        for run_id in 1..=4 {
            fs::create_dir_all(ci_root.join(format!("{run_id:04}"))).expect("run dir");
        }

        prune_ci_runs(&ci_root, 2, Some(&ci_root.join("0001"))).expect("prune ci runs");
        assert!(ci_root.join("0001").is_dir());
        assert!(!ci_root.join("0002").exists());
        assert!(!ci_root.join("0003").exists());
        assert!(ci_root.join("0004").is_dir());

        let _ = fs::remove_dir_all(temp);
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("{prefix}-{}-{now}", std::process::id()))
    }
}
