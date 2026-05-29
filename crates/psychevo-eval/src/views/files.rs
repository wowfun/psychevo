#[allow(unused_imports)]
use super::*;

pub(crate) fn build_analysis_report(cell: &CellRun) -> ViewAnalysisReport {
    let json = artifact_file_from_relative(
        &cell.cell_root,
        Path::new("analysis.json"),
        ArtifactFileMode::Analysis,
    )
    .ok()
    .flatten();
    let status = if json.is_some() { "cached" } else { "missing" }.to_string();
    let summary = json
        .as_ref()
        .and_then(|file| file.preview.as_deref())
        .and_then(analysis_summary_from_preview);
    ViewAnalysisReport {
        trial_key: trial_key(cell),
        status,
        json_ref: json.as_ref().map(|file| file.data_ref.clone()),
        json_preview: json.and_then(|file| file.preview),
        summary,
    }
}

pub(crate) fn build_diff_report(cell: &CellRun) -> ViewDiffReport {
    match discover_patch_artifacts(&cell.cell_root) {
        Ok(Some(file)) => {
            return ViewDiffReport {
                trial_key: trial_key(cell),
                source: "artifact".to_string(),
                data_ref: Some(file.data_ref),
                preview: file.preview,
                truncated: file.truncated,
                error: None,
            };
        }
        Ok(None) => {}
        Err(err) => {
            return ViewDiffReport {
                trial_key: trial_key(cell),
                source: "error".to_string(),
                data_ref: None,
                preview: None,
                truncated: false,
                error: Some(format!("{err:#}")),
            };
        }
    }
    match read_trajectory_events(cell) {
        Ok(events) => {
            let mut diffs = Vec::new();
            for event in &events {
                collect_diff_strings(&event.data, &mut diffs);
            }
            if let Some(diff) = diffs.into_iter().next() {
                let redacted = redact_preview_text(&diff);
                let (preview, truncated) =
                    truncate_chars_with_flag(&redacted, VIEW_TEXT_PREVIEW_BYTES);
                ViewDiffReport {
                    trial_key: trial_key(cell),
                    source: "trajectory".to_string(),
                    data_ref: data_ref_for_relative(
                        &cell.cell_root,
                        &cell.case.artifacts.trajectory,
                        None,
                    )
                    .ok(),
                    preview: Some(preview),
                    truncated,
                    error: None,
                }
            } else {
                ViewDiffReport {
                    trial_key: trial_key(cell),
                    source: "missing".to_string(),
                    data_ref: None,
                    preview: None,
                    truncated: false,
                    error: None,
                }
            }
        }
        Err(err) => ViewDiffReport {
            trial_key: trial_key(cell),
            source: "error".to_string(),
            data_ref: None,
            preview: None,
            truncated: false,
            error: Some(format!("{err:#}")),
        },
    }
}

#[derive(Clone, Copy)]
pub(crate) enum ArtifactFileMode {
    Artifact,
    Analysis,
    Diff,
}

pub(crate) fn list_artifact_files(
    root: &Path,
    mode: ArtifactFileMode,
) -> Result<Vec<ViewArtifactFile>> {
    list_artifact_files_under(root, root, mode)
}

pub(crate) fn list_artifact_files_under(
    root: &Path,
    start: &Path,
    mode: ArtifactFileMode,
) -> Result<Vec<ViewArtifactFile>> {
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_start = fs::canonicalize(start)
        .with_context(|| format!("failed to canonicalize {}", start.display()))?;
    if !canonical_start.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_start.display(),
            canonical_root.display()
        );
    }
    let mut out = Vec::new();
    collect_artifact_files(&canonical_root, &canonical_start, mode, &mut out)?;
    out.sort_by(|left, right| {
        left.data_ref
            .relative_path
            .cmp(&right.data_ref.relative_path)
    });
    Ok(out)
}

pub(crate) fn collect_artifact_files(
    canonical_root: &Path,
    dir: &Path,
    mode: ArtifactFileMode,
    out: &mut Vec<ViewArtifactFile>,
) -> Result<()> {
    for entry in fs::read_dir(dir).with_context(|| format!("failed to read {}", dir.display()))? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_artifact_files(canonical_root, &path, mode, out)?;
        } else if file_type.is_file() {
            out.push(artifact_file_from_canonical(canonical_root, &path, mode)?);
        }
    }
    Ok(())
}

pub(crate) fn artifact_file_from_relative(
    root: &Path,
    relative: &Path,
    mode: ArtifactFileMode,
) -> Result<Option<ViewArtifactFile>> {
    let path = root.join(relative);
    if !path.exists() || !path.is_file() {
        return Ok(None);
    }
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_path = fs::canonicalize(&path)
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    Ok(Some(artifact_file_from_canonical(
        &canonical_root,
        &canonical_path,
        mode,
    )?))
}

pub(crate) fn artifact_file_from_canonical(
    canonical_root: &Path,
    canonical_path: &Path,
    mode: ArtifactFileMode,
) -> Result<ViewArtifactFile> {
    if !canonical_path.starts_with(canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    let relative = canonical_path
        .strip_prefix(canonical_root)
        .with_context(|| {
            format!(
                "failed to relativize {} under {}",
                canonical_path.display(),
                canonical_root.display()
            )
        })?
        .to_path_buf();
    let mut data_ref = data_ref_for_canonical(canonical_path, &relative)?;
    data_ref.kind = artifact_kind(&relative);
    data_ref.label = relative.display().to_string();
    let is_image = data_ref.mime.starts_with("image/");
    let allow_preview = matches!(mode, ArtifactFileMode::Analysis | ArtifactFileMode::Diff);
    let (preview, truncated, previewable) = if allow_preview {
        read_text_preview(canonical_path)?
    } else {
        (None, false, false)
    };
    let inline_data_url = if matches!(mode, ArtifactFileMode::Artifact)
        && is_image
        && data_ref.size_bytes <= SMALL_IMAGE_INLINE_BYTES
    {
        inline_file_data_url(canonical_path, &data_ref.mime)
            .ok()
            .flatten()
    } else {
        None
    };
    Ok(ViewArtifactFile {
        data_ref,
        previewable,
        truncated,
        preview,
        inline_data_url,
    })
}

pub(crate) fn data_ref_for_relative(
    root: &Path,
    relative: &Path,
    kind_override: Option<&str>,
) -> Result<ViewDataRef> {
    let path = root.join(relative);
    let canonical_root = fs::canonicalize(root)
        .with_context(|| format!("failed to canonicalize {}", root.display()))?;
    let canonical_path = fs::canonicalize(&path)
        .with_context(|| format!("failed to canonicalize {}", path.display()))?;
    if !canonical_path.starts_with(&canonical_root) {
        bail!(
            "artifact path {} escapes cell root {}",
            canonical_path.display(),
            canonical_root.display()
        );
    }
    let relative = canonical_path
        .strip_prefix(canonical_root)
        .with_context(|| format!("failed to relativize {}", canonical_path.display()))?;
    let mut data_ref = data_ref_for_canonical(&canonical_path, relative)?;
    if let Some(kind) = kind_override {
        data_ref.kind = kind.to_string();
    }
    Ok(data_ref)
}

pub(crate) fn data_ref_for_canonical(
    canonical_path: &Path,
    relative: &Path,
) -> Result<ViewDataRef> {
    let metadata = fs::metadata(canonical_path)
        .with_context(|| format!("failed to stat {}", canonical_path.display()))?;
    let content_hash = if metadata.len() <= VIEW_TEXT_PREVIEW_BYTES as u64 {
        fs::read(canonical_path)
            .ok()
            .map(|bytes| stable_hash_bytes(&bytes))
    } else {
        None
    };
    let modified_ms = metadata
        .modified()
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis());
    Ok(ViewDataRef {
        kind: artifact_kind(relative),
        label: relative.display().to_string(),
        relative_path: relative.to_path_buf(),
        mime: mime_for_path(relative).to_string(),
        size_bytes: metadata.len(),
        content_hash,
        modified_ms,
    })
}

pub(crate) fn missing_data_ref(kind: &str, relative: &Path) -> ViewDataRef {
    ViewDataRef {
        kind: kind.to_string(),
        label: relative.display().to_string(),
        relative_path: relative.to_path_buf(),
        mime: mime_for_path(relative).to_string(),
        size_bytes: 0,
        content_hash: None,
        modified_ms: None,
    }
}

pub(crate) fn mime_for_path(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())
        .as_deref()
    {
        Some("html") | Some("htm") => "text/html",
        Some("md") | Some("markdown") => "text/markdown",
        Some("json") => "application/json",
        Some("jsonl") => "application/jsonl",
        Some("txt") | Some("log") => "text/plain",
        Some("diff") | Some("patch") => "text/x-diff",
        Some("png") => "image/png",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        _ => "application/octet-stream",
    }
}

pub(crate) fn inline_file_data_url(path: &Path, mime: &str) -> Result<Option<String>> {
    let bytes = fs::read(path).with_context(|| format!("failed to read {}", path.display()))?;
    if bytes.len() as u64 > SMALL_IMAGE_INLINE_BYTES {
        return Ok(None);
    }
    Ok(Some(format!(
        "data:{mime};base64,{}",
        BASE64_STANDARD.encode(bytes)
    )))
}

pub(crate) fn read_text_preview(path: &Path) -> Result<(Option<String>, bool, bool)> {
    let mut file =
        fs::File::open(path).with_context(|| format!("failed to open {}", path.display()))?;
    let mut bytes = Vec::new();
    let limit = VIEW_TEXT_PREVIEW_BYTES + 1;
    file.by_ref()
        .take(limit as u64)
        .read_to_end(&mut bytes)
        .with_context(|| format!("failed to read {}", path.display()))?;
    let truncated = bytes.len() > VIEW_TEXT_PREVIEW_BYTES;
    if truncated {
        bytes.truncate(VIEW_TEXT_PREVIEW_BYTES);
    }
    if bytes.contains(&0) {
        return Ok((None, truncated, false));
    }
    let Ok(text) = String::from_utf8(bytes) else {
        return Ok((None, truncated, false));
    };
    Ok((Some(redact_preview_text(&text)), truncated, true))
}

pub(crate) fn artifact_kind(path: &Path) -> String {
    if path == Path::new("run.json") {
        return "run".to_string();
    }
    if path == Path::new("trajectory.jsonl") {
        return "trajectory".to_string();
    }
    if path == Path::new("prompt.md") || path == Path::new("workspace/.peval/prompt.md") {
        return "prompt".to_string();
    }
    if path == Path::new("evaluator.stdout") || path == Path::new("evaluator.stderr") {
        return "verifier-log".to_string();
    }
    if path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == "logs")
    {
        return "log".to_string();
    }
    if path
        .components()
        .next()
        .is_some_and(|component| component.as_os_str() == "workspace")
    {
        return "workspace".to_string();
    }
    match path.extension().and_then(|value| value.to_str()) {
        Some("diff") | Some("patch") => "diff".to_string(),
        Some("md") => "markdown".to_string(),
        Some("json") | Some("jsonl") => "json".to_string(),
        Some("png") | Some("jpg") | Some("jpeg") | Some("gif") | Some("webp") | Some("bmp") => {
            "image".to_string()
        }
        _ => "file".to_string(),
    }
}
pub(crate) fn discover_patch_artifacts(root: &Path) -> Result<Option<ViewArtifactFile>> {
    let files = list_artifact_files(root, ArtifactFileMode::Diff)?;
    Ok(files.into_iter().find(|file| {
        file.data_ref
            .relative_path
            .extension()
            .and_then(|value| value.to_str())
            .is_some_and(|ext| ext == "diff" || ext == "patch")
    }))
}

pub(crate) fn collect_diff_strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            for (key, value) in map {
                if key == "diff" {
                    if let Some(diff) = value.as_str() {
                        out.push(diff.to_string());
                    }
                } else {
                    collect_diff_strings(value, out);
                }
            }
        }
        Value::Array(values) => {
            for value in values {
                collect_diff_strings(value, out);
            }
        }
        _ => {}
    }
}
