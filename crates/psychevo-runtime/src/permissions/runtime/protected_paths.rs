pub(crate) fn protected_write_reason(target: &FileTarget) -> Option<String> {
    let rel = target.relative.as_str();
    let rel_lower = rel.to_ascii_lowercase();
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if rel == ".psychevo/config.toml" {
        return Some("permission configuration cannot be modified by model tools".to_string());
    }
    if file_name == ".env" {
        return Some("protected credential file write denied".to_string());
    }
    let protected_files = [
        ".bashrc",
        ".zshrc",
        ".profile",
        ".bash_profile",
        ".zprofile",
        ".netrc",
        ".pgpass",
        ".npmrc",
        ".pypirc",
    ];
    if protected_files.contains(&file_name) {
        return Some(format!("protected file write denied: {file_name}"));
    }
    let protected_dirs = [
        ".ssh/",
        ".aws/",
        ".gnupg/",
        ".kube/",
        ".docker/",
        ".azure/",
        ".config/gh/",
    ];
    if protected_dirs
        .iter()
        .any(|prefix| rel_lower == prefix.trim_end_matches('/') || rel_lower.starts_with(prefix))
    {
        return Some("protected credential directory write denied".to_string());
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
