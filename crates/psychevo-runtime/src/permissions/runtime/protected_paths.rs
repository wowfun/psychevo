pub(crate) fn protected_permission_config_reason(
    action: &PermissionAction,
    protected_paths: &[PathBuf],
) -> Option<String> {
    let PermissionAction::File {
        paths,
        mutating: true,
        ..
    } = action
    else {
        return None;
    };
    paths
        .iter()
        .any(|target| protected_paths.contains(&target.absolute))
        .then(|| "active Psychevo permission configuration cannot be modified by model tools".to_string())
}

pub(crate) fn protected_write_reason(target: &FileTarget) -> Option<String> {
    let rel = target.relative.as_str();
    if rel == ".psychevo/config.toml" {
        return Some("permission configuration cannot be modified by model tools".to_string());
    }
    None
}

pub(crate) fn protected_read_reason(target: &FileTarget) -> Option<String> {
    let rel = target.relative.to_ascii_lowercase();
    if rel.starts_with(".psychevo/skills/.hub/") || rel.starts_with(".psychevo/cache/") {
        return Some("internal Psychevo cache files cannot be read directly".to_string());
    }
    None
}
