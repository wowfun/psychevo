#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PermissionAction {
    ExecCommand {
        command: String,
        normalized: String,
        cwd: Option<FileTarget>,
    },
    File {
        tool: String,
        paths: Vec<FileTarget>,
        mutating: bool,
    },
    Skill {
        tool: String,
        action: String,
    },
    McpStartup {
        server: String,
        transport: String,
    },
    Mcp {
        server: String,
        tool: String,
    },
    WebFetch {
        url: String,
    },
    WebSearch {
        query: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileTarget {
    pub(crate) raw: String,
    pub(crate) requested_absolute: PathBuf,
    pub(crate) absolute: PathBuf,
    pub(crate) uri: String,
    pub(crate) relative: String,
    pub(crate) within_cwd: bool,
}

impl PermissionAction {
    pub(crate) fn from_tool_call(
        cwd: &Path,
        tool_name: &str,
        args: &Value,
    ) -> crate::error::Result<Option<Self>> {
        let action = match tool_name {
            "exec_command" => {
                let Some(command) = args.get("cmd").and_then(Value::as_str) else {
                    return Ok(None);
                };
                Some(Self::ExecCommand {
                        command: command.to_string(),
                        normalized: normalize_command(command),
                        cwd: match args.get("cwd").and_then(Value::as_str) {
                            Some(path) => Some(file_target(cwd, path)?),
                            None => None,
                        },
                    })
            }
            "read" => file_paths_from_args(cwd, args, &["path"])?.map(|paths| Self::File {
                tool: "read".to_string(),
                paths,
                mutating: false,
            }),
            "write" => file_paths_from_args(cwd, args, &["path"])?.map(|paths| Self::File {
                tool: "write".to_string(),
                paths,
                mutating: true,
            }),
            "edit" => {
                let paths = edit_paths_from_args(cwd, args)?;
                (!paths.is_empty()).then(|| Self::File {
                    tool: "edit".to_string(),
                    paths,
                    mutating: true,
                })
            }
            "skill_manage" => {
                args.get("action")
                    .and_then(Value::as_str)
                    .map(|action| Self::Skill {
                        tool: "skill_manage".to_string(),
                        action: action.to_string(),
                    })
            }
            "skill_hub" => args
                .get("action")
                .and_then(Value::as_str)
                .and_then(|action| {
                    (!matches!(
                        action,
                        "browse" | "search" | "inspect" | "list" | "check" | "audit"
                    ))
                    .then(|| Self::Skill {
                        tool: "skill_hub".to_string(),
                        action: action.to_string(),
                    })
                }),
            "skill_config" => args
                .get("action")
                .and_then(Value::as_str)
                .and_then(|action| {
                    (action != "status").then(|| Self::Skill {
                        tool: "skill_config".to_string(),
                        action: action.to_string(),
                    })
                }),
            "web_fetch" => args
                .get("url")
                .and_then(Value::as_str)
                .map(|url| Self::WebFetch {
                    url: url.to_string(),
                }),
            "web_search" => args
                .get("query")
                .and_then(Value::as_str)
                .map(|query| Self::WebSearch { query: query.to_string() }),
            "mcp_startup" => {
                args.get("server")
                    .and_then(Value::as_str)
                    .map(|server| Self::McpStartup {
                        server: server.to_string(),
                        transport: args
                            .get("transport")
                            .and_then(Value::as_str)
                            .unwrap_or("unknown")
                            .to_string(),
                    })
            }
            _ => crate::mcp::mcp_utility_action(tool_name, args)
                .or_else(|| {
                    crate::mcp::mcp_tool_name_parts(tool_name)
                        .map(|(server, tool)| (server.to_string(), tool.to_string()))
                })
                .map(|(server, tool)| Self::Mcp { server, tool }),
        };
        Ok(action)
    }

    #[allow(dead_code)]
    pub(crate) fn matches_rule(&self, rule: &PermissionRule) -> bool {
        match self {
            Self::ExecCommand { normalized, .. } => {
                rule.tool == "exec_command" && wildcard_match(&rule.pattern, normalized)
            }
            Self::File { tool, paths, .. } => {
                rule.tool == *tool
                    && paths.iter().any(|target| {
                        if Path::new(&rule.pattern).is_absolute() {
                            wildcard_match(&rule.pattern, &target.absolute.to_string_lossy())
                        } else {
                            wildcard_match(&rule.pattern, &target.relative)
                        }
                    })
            }
            Self::Skill { tool, action } => {
                rule.tool == *tool && wildcard_match(&rule.pattern, action)
            }
            Self::McpStartup { server, .. } => {
                rule.tool == "mcp_startup" && wildcard_match(&rule.pattern, server)
            }
            Self::Mcp { server, tool } => {
                rule.tool == "mcp" && wildcard_match(&rule.pattern, &format!("{server}/{tool}"))
            }
            Self::WebFetch { url } => {
                rule.tool == "web_fetch" && wildcard_match(&rule.pattern, url)
            }
            Self::WebSearch { query } => {
                rule.tool == "web_search" && wildcard_match(&rule.pattern, query)
            }
        }
    }

    pub(crate) fn session_key(&self) -> String {
        match self {
            Self::ExecCommand {
                command,
                normalized,
                ..
            } => exec_grant_prefix(command)
                .map(|prefix| format!("exec_policy:{}", prefix.join(" ")))
                .unwrap_or_else(|| format!("exec_command:{normalized}")),
            Self::File { tool, paths, .. } => format!(
                "{tool}:{}",
                paths
                    .iter()
                    .map(|target| target.absolute.to_string_lossy().to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Skill { tool, action } => format!("{tool}:{action}"),
            Self::McpStartup { server, .. } => format!("mcp_startup:{server}"),
            Self::Mcp { server, tool } => format!("mcp:{server}/{tool}"),
            Self::WebFetch { url } => format!("web_fetch:{url}"),
            Self::WebSearch { query } => format!("web_search:{query}"),
        }
    }

    pub(crate) fn suggested_rule(&self) -> Option<String> {
        match self {
            Self::ExecCommand { command, .. } => exec_grant_prefix(command)
                .map(|prefix| format!("exec:{}", prefix.join(" ")))
                .or_else(|| Some(format!("ExecCommand({command})"))),
            Self::File { .. } => None,
            Self::Skill { tool, action } => {
                Some(format!("{}({action})", permission_rule_tool(tool)))
            }
            Self::McpStartup { server, .. } => Some(format!("McpStartup({server})")),
            Self::Mcp { server, tool } => Some(format!("Mcp({server}/{tool})")),
            Self::WebFetch { url } => Some(format!("WebFetch({url})")),
            Self::WebSearch { query } => Some(format!("WebSearch({query})")),
        }
    }

    #[allow(dead_code)]
    pub(crate) fn allow_always(&self) -> bool {
        matches!(
            self,
            Self::ExecCommand { .. }
                | Self::Skill { .. }
                | Self::McpStartup { .. }
                | Self::Mcp { .. }
                | Self::WebFetch { .. }
                | Self::WebSearch { .. }
        )
    }

    pub(crate) fn persistent_grants(&self) -> Vec<PersistentPermissionGrant> {
        match self {
            Self::ExecCommand { command, .. } => {
                let prefix = exec_grant_prefix(command).unwrap_or_else(|| command_tokens(command));
                (!prefix.is_empty())
                    .then_some(PersistentPermissionGrant::Exec {
                        prefix,
                        decision: ExecPolicyDecision::Allow,
                    })
                    .into_iter()
                    .collect()
            }
            Self::File { .. } => Vec::new(),
            Self::Skill { tool, action } => vec![PersistentPermissionGrant::Skill {
                key: format!("{tool}/{action}"),
                access: PermissionAccess::Allow,
            }],
            Self::WebFetch { url } => web_fetch_host(url)
                .map(|host| {
                    vec![PersistentPermissionGrant::Network {
                        host,
                        access: PermissionAccess::Allow,
                    }]
                })
                .unwrap_or_default(),
            Self::WebSearch { query } => vec![PersistentPermissionGrant::WebSearch {
                query: query.clone(),
                access: PermissionAccess::Allow,
            }],
            Self::McpStartup { .. } | Self::Mcp { .. } => Vec::new(),
        }
    }

    pub(crate) fn file_targets_all_within_cwd(&self) -> bool {
        match self {
            Self::File { paths, .. } => paths.iter().all(|path| path.within_cwd),
            _ => false,
        }
    }

    pub(crate) fn filesystem_identity_snapshot(&self) -> Option<Vec<PathBuf>> {
        match self {
            Self::File { paths, .. } => {
                Some(paths.iter().map(|target| target.absolute.clone()).collect())
            }
            Self::ExecCommand { cwd: Some(cwd), .. } => Some(vec![cwd.absolute.clone()]),
            _ => None,
        }
    }

    pub(crate) fn filesystem_approval_request(&self) -> Option<FilesystemApprovalRequest> {
        let Self::File {
            paths,
            mutating: true,
            ..
        } = self
        else {
            return None;
        };
        let targets = paths
            .iter()
            .map(|target| FilesystemApprovalTarget {
                requested_path: target.raw.clone(),
                resolved_path: target.absolute.to_string_lossy().to_string(),
            })
            .collect();
        let scope_candidates = common_scope_candidates(paths);
        Some(FilesystemApprovalRequest {
            targets,
            scope_candidates,
        })
    }

    pub(crate) fn category(&self) -> &'static str {
        match self {
            Self::ExecCommand { .. } => "exec",
            Self::File { .. } => "filesystem",
            Self::Skill { .. } => "skill",
            Self::McpStartup { .. } | Self::Mcp { .. } => "mcp",
            Self::WebFetch { .. } => "network",
            Self::WebSearch { .. } => "network",
        }
    }

    pub(crate) fn is_safe_file_edit(&self) -> bool {
        matches!(
            self,
            Self::File {
                mutating: true,
                paths,
                ..
            } if paths.iter().all(|path| protected_write_reason(path).is_none())
        )
    }
}

fn common_scope_candidates(paths: &[FileTarget]) -> Vec<String> {
    let Some(mut candidate) = paths
        .first()
        .and_then(|target| target.absolute.parent())
        .map(Path::to_path_buf)
    else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    loop {
        if paths
            .iter()
            .all(|target| crate::filesystem_identity::is_within(&candidate, &target.absolute))
        {
            candidates.push(candidate.to_string_lossy().to_string());
        }
        let Some(parent) = candidate.parent().map(Path::to_path_buf) else {
            break;
        };
        candidate = parent;
    }
    candidates
}

pub(crate) fn file_paths_from_args(
    cwd: &Path,
    args: &Value,
    keys: &[&str],
) -> crate::error::Result<Option<Vec<FileTarget>>> {
    let paths = keys
        .iter()
        .filter_map(|key| args.get(*key).and_then(Value::as_str))
        .map(|path| file_target(cwd, path))
        .collect::<crate::error::Result<Vec<_>>>()?;
    Ok((!paths.is_empty()).then_some(paths))
}

pub(crate) fn edit_paths_from_args(
    cwd: &Path,
    args: &Value,
) -> crate::error::Result<Vec<FileTarget>> {
    if let Some(paths) = file_paths_from_args(cwd, args, &["path"])? {
        return Ok(paths);
    }
    args.get("patch")
        .and_then(Value::as_str)
        .map(|patch| {
            patch
                .lines()
                .flat_map(patch_file_paths)
                .map(|path| file_target(cwd, &path))
                .collect::<crate::error::Result<Vec<_>>>()
        })
        .unwrap_or_else(|| Ok(Vec::new()))
}

pub(crate) fn patch_file_paths(line: &str) -> Vec<String> {
    let line = line.trim();
    for marker in [
        "*** Update File:",
        "*** Add File:",
        "*** Delete File:",
        "*** Move to:",
    ] {
        if let Some(path) = line.strip_prefix(marker) {
            return vec![path.trim().to_string()];
        }
    }
    if let Some(rest) = line.strip_prefix("*** Move File:") {
        if let Some((from, to)) = rest.split_once("->") {
            return vec![from.trim().to_string(), to.trim().to_string()];
        }
        return vec![rest.trim().to_string()];
    }
    Vec::new()
}

pub(crate) fn file_target(cwd: &Path, raw: &str) -> crate::error::Result<FileTarget> {
    let identity = crate::filesystem_identity::resolve(raw, cwd)?;
    let cwd = crate::filesystem_identity::canonicalize_deepest_existing(cwd)?;
    let within_cwd = crate::filesystem_identity::is_within(&cwd, &identity.resolved);
    let relative = relative_path_from(&cwd, &identity.resolved)
        .unwrap_or_else(|| identity.resolved.clone())
        .to_string_lossy()
        .replace('\\', "/");
    Ok(FileTarget {
        raw: raw.to_string(),
        requested_absolute: identity.requested_absolute,
        absolute: identity.resolved,
        uri: identity.uri,
        relative,
        within_cwd,
    })
}

fn relative_path_from(base: &Path, target: &Path) -> Option<PathBuf> {
    let base = base.components().collect::<Vec<_>>();
    let target = target.components().collect::<Vec<_>>();
    let common = base
        .iter()
        .zip(&target)
        .take_while(|(left, right)| left == right)
        .count();
    let mut relative = PathBuf::new();
    for component in &base[common..] {
        match component {
            std::path::Component::Normal(_) | std::path::Component::ParentDir => {
                relative.push("..")
            }
            std::path::Component::CurDir => {}
            std::path::Component::Prefix(_) | std::path::Component::RootDir => return None,
        }
    }
    for component in &target[common..] {
        match component {
            std::path::Component::Normal(value) => relative.push(value),
            std::path::Component::ParentDir => relative.push(".."),
            std::path::Component::CurDir => {}
            std::path::Component::Prefix(_) | std::path::Component::RootDir => return None,
        }
    }
    Some(relative)
}

#[cfg(test)]
mod action_path_tests {
    use super::*;

    #[test]
    fn file_target_relative_path_preserves_spaces() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path();

        let target = file_target(cwd, "a b.txt").expect("target");

        assert_eq!(target.relative, "a b.txt");
        assert!(target.within_cwd);
    }

    #[test]
    fn file_target_file_uri_relative_path_is_decoded() {
        let temp = tempfile::tempdir().expect("temp");
        let cwd = temp.path();
        let path = cwd.join("a b.txt");
        let path_ref = crate::host_paths::path_ref_for_native_path(&path);

        let target = file_target(cwd, &path_ref.uri).expect("target");

        assert_eq!(target.relative, "a b.txt");
        assert!(target.within_cwd);
        assert!(target.uri.ends_with("/a%20b.txt"));
    }
}
