#[allow(unused_imports)]
use super::*;

pub(crate) fn read_trajectory_events(cell: &CellRun) -> Result<Vec<TrajectoryEvent>> {
    let path = cell.cell_root.join(&cell.case.artifacts.trajectory);
    let safe = safe_artifact_path(&cell.cell_root, &path)?;
    let file = fs::File::open(&safe)
        .with_context(|| format!("failed to open trajectory {}", safe.display()))?;
    let mut events = Vec::new();
    for (line_no, line) in BufReader::new(file).lines().enumerate() {
        let line = line.with_context(|| format!("failed to read {}", safe.display()))?;
        if line.trim().is_empty() {
            continue;
        }
        let event = serde_json::from_str::<TrajectoryEvent>(&line).with_context(|| {
            format!(
                "failed to parse trajectory event {} line {}",
                safe.display(),
                line_no + 1
            )
        })?;
        events.push(event);
    }
    Ok(events)
}

pub(crate) fn safe_artifact_path(root: &Path, path: &Path) -> Result<PathBuf> {
    let joined = if path.is_absolute() {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_path = fs::canonicalize(&joined)
        .with_context(|| format!("failed to canonicalize {}", joined.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    Ok(canonical_path)
}
