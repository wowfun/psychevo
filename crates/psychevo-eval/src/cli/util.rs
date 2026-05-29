#[allow(unused_imports)]
use crate::*;

pub(crate) fn looks_like_relative_path(value: &str) -> bool {
    if value.starts_with('{') || value.contains('\n') {
        return false;
    }
    let path = Path::new(value);
    !path.is_absolute()
        && (value.starts_with("./")
            || value.starts_with("../")
            || value.contains('/')
            || value.contains('\\'))
}

pub(crate) fn is_declared_path(value: &str, task_dir: &Path) -> bool {
    task_dir.join(value).exists() || looks_like_relative_path(value)
}

pub(crate) fn sanitize_id(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

pub(crate) fn slugify(value: &str) -> String {
    let slug = sanitize_id(&value.to_ascii_lowercase())
        .trim_matches('_')
        .to_string();
    if slug.is_empty() {
        "evaluation".to_string()
    } else {
        slug
    }
}

pub(crate) fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn reject_unsupported(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != MANIFEST_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported schema_version {}; supported schema_version is {}. v4 benchmark, eval, and task manifests are no longer supported; see docs/evaluation/authoring.md for v5 authoring.",
            path.display(),
            schema_version,
            MANIFEST_SCHEMA_VERSION
        );
    }
    Ok(())
}

pub(crate) fn reject_unsupported_artifact(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != ARTIFACT_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported artifact schema_version {}; supported artifact schema_version is {}",
            path.display(),
            schema_version,
            ARTIFACT_SCHEMA_VERSION
        );
    }
    Ok(())
}

pub(crate) fn reject_unsupported_index(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != INDEX_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported index schema_version {}; supported index schema_version is {}",
            path.display(),
            schema_version,
            INDEX_SCHEMA_VERSION
        );
    }
    Ok(())
}

pub(crate) fn reject_unsupported_workspace(schema_version: u32, path: &Path) -> Result<()> {
    if schema_version != WORKSPACE_SCHEMA_VERSION {
        bail!(
            "{} uses unsupported workspace schema_version {}; supported workspace schema_version is {}",
            path.display(),
            schema_version,
            WORKSPACE_SCHEMA_VERSION
        );
    }
    Ok(())
}
