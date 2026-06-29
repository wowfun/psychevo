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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct FileTarget {
    pub(crate) raw: String,
    pub(crate) absolute: PathBuf,
    pub(crate) relative: String,
    pub(crate) within_cwd: bool,
}

impl PermissionAction {
    pub(crate) fn from_tool_call(cwd: &Path, tool_name: &str, args: &Value) -> Option<Self> {
        match tool_name {
            "exec_command" => {
                args.get("cmd")
                    .and_then(Value::as_str)
                    .map(|command| Self::ExecCommand {
                        command: command.to_string(),
                        normalized: normalize_command(command),
                        cwd: args
                            .get("cwd")
                            .and_then(Value::as_str)
                            .map(|path| file_target(cwd, path)),
                    })
            }
            "read" => file_paths_from_args(cwd, args, &["path"]).map(|paths| Self::File {
                tool: "read".to_string(),
                paths,
                mutating: false,
            }),
            "write" => file_paths_from_args(cwd, args, &["path"]).map(|paths| Self::File {
                tool: "write".to_string(),
                paths,
                mutating: true,
            }),
            "edit" => {
                let paths = edit_paths_from_args(cwd, args);
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
            _ => crate::mcp::mcp_tool_name_parts(tool_name).map(|(server, tool)| Self::Mcp {
                server: server.to_string(),
                tool: tool.to_string(),
            }),
        }
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
                    .map(|target| target.relative.clone())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Self::Skill { tool, action } => format!("{tool}:{action}"),
            Self::McpStartup { server, .. } => format!("mcp_startup:{server}"),
            Self::Mcp { server, tool } => format!("mcp:{server}/{tool}"),
            Self::WebFetch { url } => format!("web_fetch:{url}"),
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
            Self::File {
                paths, mutating, ..
            } => {
                let access = if *mutating {
                    PermissionAccess::Write
                } else {
                    PermissionAccess::Read
                };
                paths
                    .iter()
                    .map(|target| PersistentPermissionGrant::Filesystem {
                        path: target.absolute.to_string_lossy().to_string(),
                        access,
                    })
                    .collect()
            }
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
            Self::McpStartup { .. } | Self::Mcp { .. } => Vec::new(),
        }
    }

    pub(crate) fn file_targets_all_within_cwd(&self) -> bool {
        match self {
            Self::File { paths, .. } => paths.iter().all(|path| path.within_cwd),
            _ => false,
        }
    }

    pub(crate) fn category(&self) -> &'static str {
        match self {
            Self::ExecCommand { .. } => "exec",
            Self::File { .. } => "filesystem",
            Self::Skill { .. } => "skill",
            Self::McpStartup { .. } | Self::Mcp { .. } => "mcp",
            Self::WebFetch { .. } => "network",
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

pub(crate) fn file_paths_from_args(
    cwd: &Path,
    args: &Value,
    keys: &[&str],
) -> Option<Vec<FileTarget>> {
    let paths = keys
        .iter()
        .filter_map(|key| args.get(*key).and_then(Value::as_str))
        .map(|path| file_target(cwd, path))
        .collect::<Vec<_>>();
    (!paths.is_empty()).then_some(paths)
}

pub(crate) fn edit_paths_from_args(cwd: &Path, args: &Value) -> Vec<FileTarget> {
    if let Some(paths) = file_paths_from_args(cwd, args, &["path"]) {
        return paths;
    }
    args.get("patch")
        .and_then(Value::as_str)
        .map(|patch| {
            patch
                .lines()
                .flat_map(patch_file_paths)
                .map(|path| file_target(cwd, &path))
                .collect()
        })
        .unwrap_or_default()
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

pub(crate) fn file_target(cwd: &Path, raw: &str) -> FileTarget {
    let path = Path::new(raw);
    let absolute = if path.is_absolute() {
        lexical_normalize(path)
    } else {
        lexical_normalize(&cwd.join(path))
    };
    let (relative, within_cwd) = absolute
        .strip_prefix(cwd)
        .map(|path| (path.to_string_lossy().replace('\\', "/"), true))
        .unwrap_or_else(|_| (raw.replace('\\', "/"), false));
    FileTarget {
        raw: raw.to_string(),
        absolute,
        relative,
        within_cwd,
    }
}

pub(crate) fn lexical_normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}
