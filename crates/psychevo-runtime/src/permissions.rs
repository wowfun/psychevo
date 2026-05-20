use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::future::BoxFuture;
use psychevo_agent_core::{ToolBinding, ToolExecutionMode, ToolOutput};
use psychevo_ai::AbortSignal;
use serde_json::{Value, json};
use tokio::time;

use crate::config::append_local_permission_allow_rule;
use crate::types::{
    ApprovalMode, PermissionApprovalOutcome, PermissionApprovalRequest, PermissionConfig,
    PermissionMode,
};

#[derive(Clone)]
pub(crate) struct PermissionRuntime {
    inner: Arc<PermissionRuntimeInner>,
}

struct PermissionRuntimeInner {
    workdir: PathBuf,
    project_config_dir: PathBuf,
    mode: PermissionMode,
    approval_mode: ApprovalMode,
    smart_model: Option<String>,
    allow: Vec<PermissionRule>,
    ask: Vec<PermissionRule>,
    deny: Vec<PermissionRule>,
    session_grants: Mutex<HashSet<String>>,
    approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PermissionRule {
    raw: String,
    tool: String,
    pattern: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionDecision {
    Allow,
    Ask {
        reason: String,
        matched_rule: Option<String>,
        suggested_rule: Option<String>,
        allow_always: bool,
        session_key: String,
    },
    Deny {
        reason: String,
        matched_rule: Option<String>,
    },
}

impl PermissionRuntime {
    pub(crate) fn new(
        workdir: PathBuf,
        project_config_dir: PathBuf,
        config: PermissionConfig,
        mode: PermissionMode,
        approval_mode: ApprovalMode,
        approval_handler: Option<Arc<dyn crate::types::ApprovalHandler>>,
    ) -> Self {
        Self {
            inner: Arc::new(PermissionRuntimeInner {
                workdir,
                project_config_dir,
                mode,
                approval_mode,
                smart_model: config.smart_model,
                allow: parse_rules(config.allow),
                ask: parse_rules(config.ask),
                deny: parse_rules(config.deny),
                session_grants: Mutex::new(HashSet::new()),
                approval_handler,
            }),
        }
    }

    pub(crate) fn wrap_tools(&self, tools: Vec<Arc<dyn ToolBinding>>) -> Vec<Arc<dyn ToolBinding>> {
        tools
            .into_iter()
            .map(|tool| {
                Arc::new(PermissionTool {
                    tool,
                    runtime: self.clone(),
                }) as Arc<dyn ToolBinding>
            })
            .collect()
    }

    async fn authorize(
        &self,
        tool_call_id: &str,
        tool_name: &str,
        args: &Value,
    ) -> std::result::Result<(), ToolOutput> {
        match self.evaluate(tool_name, args) {
            PermissionDecision::Allow => Ok(()),
            PermissionDecision::Deny {
                reason,
                matched_rule,
            } => Err(permission_error("denied", &reason, matched_rule.as_deref())),
            PermissionDecision::Ask {
                reason,
                matched_rule,
                suggested_rule,
                allow_always,
                session_key,
            } => {
                if self.inner.mode.bypasses_prompt_asks() {
                    return Ok(());
                }
                if self.inner.mode == PermissionMode::DontAsk {
                    return Err(permission_error(
                        "denied",
                        &format!("permission prompt suppressed by dontAsk: {reason}"),
                        matched_rule.as_deref(),
                    ));
                }
                if self.inner.approval_handler.is_none()
                    || matches!(self.inner.approval_mode, ApprovalMode::Smart)
                        && self.inner.approval_handler.is_none()
                {
                    return Ok(());
                }
                let Some(handler) = &self.inner.approval_handler else {
                    return Ok(());
                };
                let timeout_secs = handler.timeout_secs();
                let request = PermissionApprovalRequest {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: tool_name.to_string(),
                    summary: action_summary(tool_name, args),
                    reason: if self.inner.approval_mode == ApprovalMode::Smart {
                        match &self.inner.smart_model {
                            Some(model) => format!("{reason} (smart reviewer configured: {model})"),
                            None => reason.clone(),
                        }
                    } else {
                        reason.clone()
                    },
                    matched_rule: matched_rule.clone(),
                    suggested_rule: suggested_rule.clone(),
                    allow_always,
                    timeout_secs,
                };
                let decision = time::timeout(
                    Duration::from_secs(timeout_secs),
                    handler.request_permission(request),
                )
                .await
                .unwrap_or_else(|_| crate::types::PermissionApprovalDecision::deny());
                match decision.outcome {
                    PermissionApprovalOutcome::AllowOnce => Ok(()),
                    PermissionApprovalOutcome::AllowSession => {
                        self.remember_session_grant(session_key);
                        Ok(())
                    }
                    PermissionApprovalOutcome::AllowAlways => {
                        self.remember_session_grant(session_key);
                        if allow_always && let Some(rule) = suggested_rule {
                            let _ = append_local_permission_allow_rule(
                                self.inner.project_config_dir.clone(),
                                &rule,
                            );
                        }
                        Ok(())
                    }
                    PermissionApprovalOutcome::Deny => {
                        Err(permission_error("denied", &reason, matched_rule.as_deref()))
                    }
                }
            }
        }
    }

    fn remember_session_grant(&self, key: String) {
        if let Ok(mut grants) = self.inner.session_grants.lock() {
            grants.insert(key);
        }
    }

    fn has_session_grant(&self, key: &str) -> bool {
        self.inner
            .session_grants
            .lock()
            .is_ok_and(|grants| grants.contains(key))
    }

    fn evaluate(&self, tool_name: &str, args: &Value) -> PermissionDecision {
        let action = PermissionAction::from_tool_call(&self.inner.workdir, tool_name, args);
        let Some(action) = action else {
            return PermissionDecision::Allow;
        };

        if let Some(reason) = hardline_deny(&action) {
            return PermissionDecision::Deny {
                reason,
                matched_rule: None,
            };
        }

        if let Some(rule) = self.matching_rule(&self.inner.deny, &action) {
            return PermissionDecision::Deny {
                reason: format!("blocked by permissions.deny rule `{}`", rule.raw),
                matched_rule: Some(rule.raw.clone()),
            };
        }

        let session_key = action.session_key();
        if self.has_session_grant(&session_key) {
            return PermissionDecision::Allow;
        }

        if let Some(rule) = self.matching_rule(&self.inner.ask, &action) {
            if self.inner.mode == PermissionMode::AcceptEdits && action.is_safe_file_edit() {
                return PermissionDecision::Allow;
            }
            return PermissionDecision::Ask {
                reason: format!("permissions.ask rule `{}` matched", rule.raw),
                matched_rule: Some(rule.raw.clone()),
                suggested_rule: action.suggested_rule(),
                allow_always: action.allow_always(),
                session_key,
            };
        }

        if let Some(_rule) = self.matching_rule(&self.inner.allow, &action) {
            return PermissionDecision::Allow;
        }

        if let Some(reason) = default_ask_reason(&action) {
            if self.inner.mode == PermissionMode::AcceptEdits && action.is_safe_file_edit() {
                return PermissionDecision::Allow;
            }
            return PermissionDecision::Ask {
                reason,
                matched_rule: None,
                suggested_rule: action.suggested_rule(),
                allow_always: action.allow_always(),
                session_key,
            };
        }

        PermissionDecision::Allow
    }

    fn matching_rule<'a>(
        &'a self,
        rules: &'a [PermissionRule],
        action: &PermissionAction,
    ) -> Option<&'a PermissionRule> {
        rules.iter().find(|rule| action.matches_rule(rule))
    }
}

struct PermissionTool {
    tool: Arc<dyn ToolBinding>,
    runtime: PermissionRuntime,
}

impl ToolBinding for PermissionTool {
    fn name(&self) -> &str {
        self.tool.name()
    }

    fn description(&self) -> &str {
        self.tool.description()
    }

    fn parameters(&self) -> Value {
        self.tool.parameters()
    }

    fn execution_mode(&self) -> ToolExecutionMode {
        self.tool.execution_mode()
    }

    fn execute(
        &self,
        tool_call_id: String,
        args: Value,
        abort: AbortSignal,
    ) -> BoxFuture<'static, ToolOutput> {
        let runtime = self.runtime.clone();
        let tool = Arc::clone(&self.tool);
        Box::pin(async move {
            if let Err(output) = runtime.authorize(&tool_call_id, tool.name(), &args).await {
                return output;
            }
            tool.execute(tool_call_id, args, abort).await
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PermissionAction {
    Bash {
        command: String,
        normalized: String,
    },
    File {
        tool: String,
        paths: Vec<FileTarget>,
        mutating: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileTarget {
    raw: String,
    absolute: PathBuf,
    relative: String,
}

impl PermissionAction {
    fn from_tool_call(workdir: &Path, tool_name: &str, args: &Value) -> Option<Self> {
        match tool_name {
            "bash" => args
                .get("command")
                .and_then(Value::as_str)
                .map(|command| Self::Bash {
                    command: command.to_string(),
                    normalized: normalize_command(command),
                }),
            "read" => file_paths_from_args(workdir, args, &["path"]).map(|paths| Self::File {
                tool: "read".to_string(),
                paths,
                mutating: false,
            }),
            "write" => file_paths_from_args(workdir, args, &["path"]).map(|paths| Self::File {
                tool: "write".to_string(),
                paths,
                mutating: true,
            }),
            "edit" => {
                let paths = edit_paths_from_args(workdir, args);
                (!paths.is_empty()).then(|| Self::File {
                    tool: "edit".to_string(),
                    paths,
                    mutating: true,
                })
            }
            _ => None,
        }
    }

    fn matches_rule(&self, rule: &PermissionRule) -> bool {
        match self {
            Self::Bash { normalized, .. } => {
                rule.tool == "bash" && wildcard_match(&rule.pattern, normalized)
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
        }
    }

    fn session_key(&self) -> String {
        match self {
            Self::Bash { normalized, .. } => format!("bash:{normalized}"),
            Self::File { tool, paths, .. } => format!(
                "{tool}:{}",
                paths
                    .iter()
                    .map(|target| target.relative.clone())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        }
    }

    fn suggested_rule(&self) -> Option<String> {
        match self {
            Self::Bash { command, .. } => Some(format!("Bash({command})")),
            Self::File { .. } => None,
        }
    }

    fn allow_always(&self) -> bool {
        matches!(self, Self::Bash { .. })
    }

    fn is_safe_file_edit(&self) -> bool {
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

fn file_paths_from_args(workdir: &Path, args: &Value, keys: &[&str]) -> Option<Vec<FileTarget>> {
    let paths = keys
        .iter()
        .filter_map(|key| args.get(*key).and_then(Value::as_str))
        .map(|path| file_target(workdir, path))
        .collect::<Vec<_>>();
    (!paths.is_empty()).then_some(paths)
}

fn edit_paths_from_args(workdir: &Path, args: &Value) -> Vec<FileTarget> {
    if let Some(paths) = file_paths_from_args(workdir, args, &["path"]) {
        return paths;
    }
    args.get("patch")
        .and_then(Value::as_str)
        .map(|patch| {
            patch
                .lines()
                .filter_map(patch_file_path)
                .map(|path| file_target(workdir, &path))
                .collect()
        })
        .unwrap_or_default()
}

fn patch_file_path(line: &str) -> Option<String> {
    let path = line
        .strip_prefix("+++ ")
        .or_else(|| line.strip_prefix("--- "))?;
    let path = path.trim();
    if path == "/dev/null" {
        return None;
    }
    Some(
        path.strip_prefix("a/")
            .or_else(|| path.strip_prefix("b/"))
            .unwrap_or(path)
            .split('\t')
            .next()
            .unwrap_or(path)
            .to_string(),
    )
}

fn file_target(workdir: &Path, raw: &str) -> FileTarget {
    let path = Path::new(raw);
    let absolute = if path.is_absolute() {
        lexical_normalize(path)
    } else {
        lexical_normalize(&workdir.join(path))
    };
    let relative = absolute
        .strip_prefix(workdir)
        .map(|path| path.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| raw.replace('\\', "/"));
    FileTarget {
        raw: raw.to_string(),
        absolute,
        relative,
    }
}

fn lexical_normalize(path: &Path) -> PathBuf {
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

fn hardline_deny(action: &PermissionAction) -> Option<String> {
    match action {
        PermissionAction::Bash { normalized, .. } => hardline_bash_reason(normalized),
        PermissionAction::File {
            paths, mutating, ..
        } => paths.iter().find_map(|target| {
            if *mutating {
                protected_write_reason(target)
            } else {
                protected_read_reason(target)
            }
        }),
    }
}

fn default_ask_reason(action: &PermissionAction) -> Option<String> {
    match action {
        PermissionAction::Bash { normalized, .. } => dangerous_bash_reason(normalized),
        PermissionAction::File { .. } => None,
    }
}

fn protected_write_reason(target: &FileTarget) -> Option<String> {
    let rel = target.relative.as_str();
    let rel_lower = rel.to_ascii_lowercase();
    let file_name = Path::new(rel)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if rel == ".psychevo/config.jsonc" {
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

fn protected_read_reason(target: &FileTarget) -> Option<String> {
    let rel = target.relative.to_ascii_lowercase();
    if rel.starts_with(".psychevo/skills/.hub/") || rel.starts_with(".psychevo/cache/") {
        return Some("internal Psychevo cache files cannot be read directly".to_string());
    }
    None
}

fn hardline_bash_reason(command: &str) -> Option<String> {
    let compact = command.replace(' ', "");
    if compact.contains("rm-rf/")
        || compact.contains("rm-fr/")
        || compact.contains("rm-rf~")
        || compact.contains("rm-rf$home")
        || compact.contains("rm-rf.")
    {
        return Some("hard-denied recursive delete target".to_string());
    }
    if command.contains("mkfs")
        || command.contains("dd if=") && command.contains(" of=/dev/")
        || command.contains(":(){")
        || command.contains("shutdown")
        || command.contains("reboot")
        || command.contains("poweroff")
        || command.contains("halt")
    {
        return Some("hard-denied system destructive command".to_string());
    }
    None
}

fn dangerous_bash_reason(command: &str) -> Option<String> {
    let dangerous = [
        ("rm -rf", "recursive delete requires approval"),
        ("rm -fr", "recursive delete requires approval"),
        ("chmod -r", "recursive chmod requires approval"),
        ("chown -r", "recursive chown requires approval"),
        (
            "git reset --hard",
            "destructive git reset requires approval",
        ),
        ("git clean -f", "destructive git clean requires approval"),
        ("git push --force", "force push requires approval"),
        ("killall", "process kill requires approval"),
        ("pkill", "process kill requires approval"),
        ("kill -9", "process kill requires approval"),
        ("systemctl", "service management requires approval"),
        ("service ", "service management requires approval"),
        ("sudo ", "sudo command requires approval"),
        ("drop table", "destructive SQL requires approval"),
        ("delete from", "destructive SQL requires approval"),
        ("truncate table", "destructive SQL requires approval"),
        ("find ", "find command requires approval when deleting"),
    ];
    if command.contains("curl") && pipe_to_shell(command)
        || command.contains("wget") && pipe_to_shell(command)
    {
        return Some("downloaded shell installer requires approval".to_string());
    }
    if command.contains("python -c")
        || command.contains("python3 -c")
        || command.contains("node -e")
        || command.contains("perl -e")
        || command.contains("ruby -e")
    {
        return Some("inline interpreter execution requires approval".to_string());
    }
    for (needle, reason) in dangerous {
        if command.contains(needle) {
            if needle == "find " && !command.contains("-delete") {
                continue;
            }
            return Some(reason.to_string());
        }
    }
    None
}

fn pipe_to_shell(command: &str) -> bool {
    command.contains("| sh")
        || command.contains("| bash")
        || command.contains("|sh")
        || command.contains("|bash")
}

fn action_summary(tool_name: &str, args: &Value) -> String {
    match tool_name {
        "bash" => args
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        "read" | "write" | "edit" => args
            .get("path")
            .and_then(Value::as_str)
            .unwrap_or("(patch)")
            .to_string(),
        _ => args.to_string(),
    }
}

fn permission_error(decision: &str, reason: &str, matched_rule: Option<&str>) -> ToolOutput {
    ToolOutput {
        json: json!({
            "error": format!("permission {decision}: {reason}"),
            "permission": {
                "decision": decision,
                "reason": reason,
                "matched_rule": matched_rule,
            }
        }),
        model_content: None,
        is_error: true,
    }
}

fn parse_rules(rules: Vec<String>) -> Vec<PermissionRule> {
    rules
        .into_iter()
        .filter_map(|raw| parse_rule(&raw))
        .collect()
}

fn parse_rule(raw: &str) -> Option<PermissionRule> {
    let raw = raw.trim();
    let (tool, rest) = raw.split_once('(')?;
    let pattern = rest.strip_suffix(')')?.trim();
    let tool = match tool.trim() {
        "Bash" | "bash" => "bash",
        "Read" | "read" => "read",
        "Write" | "write" => "write",
        "Edit" | "edit" => "edit",
        _ => return None,
    };
    Some(PermissionRule {
        raw: raw.to_string(),
        tool: tool.to_string(),
        pattern: normalize_rule_pattern(pattern, tool),
    })
}

fn normalize_rule_pattern(pattern: &str, tool: &str) -> String {
    if tool == "bash" {
        normalize_command(pattern)
    } else {
        pattern.replace('\\', "/")
    }
}

fn normalize_command(command: &str) -> String {
    command
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn wildcard_match(pattern: &str, text: &str) -> bool {
    let pattern = pattern.as_bytes();
    let text = text.as_bytes();
    let (mut p, mut t) = (0usize, 0usize);
    let mut star = None;
    let mut match_after_star = 0usize;
    while t < text.len() {
        if p < pattern.len() && (pattern[p] == b'?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == b'*' {
            star = Some(p);
            match_after_star = t;
            p += 1;
        } else if let Some(star_index) = star {
            p = star_index + 1;
            match_after_star += 1;
            t = match_after_star;
        } else {
            return false;
        }
    }
    while p < pattern.len() && pattern[p] == b'*' {
        p += 1;
    }
    p == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(config: PermissionConfig, mode: PermissionMode) -> PermissionRuntime {
        PermissionRuntime::new(
            PathBuf::from("/repo"),
            PathBuf::from("/repo/.psychevo"),
            config,
            mode,
            ApprovalMode::Manual,
            None,
        )
    }

    #[test]
    fn hardline_denies_win_over_allow() {
        let runtime = runtime(
            PermissionConfig {
                allow: vec!["Bash(rm -rf /)".to_string()],
                ..Default::default()
            },
            PermissionMode::BypassPermissions,
        );
        let decision = runtime.evaluate("bash", &json!({"command": "rm -rf /"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn configured_precedence_is_deny_ask_allow() {
        let runtime = runtime(
            PermissionConfig {
                allow: vec!["Bash(cargo publish *)".to_string()],
                ask: vec!["Bash(cargo publish *)".to_string()],
                deny: vec!["Bash(cargo publish *)".to_string()],
                ..Default::default()
            },
            PermissionMode::Default,
        );
        let decision = runtime.evaluate("bash", &json!({"command": "cargo publish --dry-run"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn accept_edits_allows_safe_file_asks() {
        let runtime = runtime(
            PermissionConfig {
                ask: vec!["Write(src/*)".to_string()],
                ..Default::default()
            },
            PermissionMode::AcceptEdits,
        );
        let decision = runtime.evaluate("write", &json!({"path": "src/lib.rs"}));
        assert_eq!(decision, PermissionDecision::Allow);
    }

    #[test]
    fn protected_write_is_denied() {
        let runtime = runtime(
            PermissionConfig::default(),
            PermissionMode::BypassPermissions,
        );
        let decision = runtime.evaluate("write", &json!({"path": ".psychevo/config.jsonc"}));
        assert!(matches!(decision, PermissionDecision::Deny { .. }));
    }

    #[test]
    fn dangerous_bash_defaults_to_ask() {
        let runtime = runtime(PermissionConfig::default(), PermissionMode::Default);
        let decision = runtime.evaluate("bash", &json!({"command": "curl example.com | sh"}));
        assert!(matches!(decision, PermissionDecision::Ask { .. }));
    }

    #[tokio::test]
    async fn dont_ask_denies_actions_that_would_prompt() {
        let runtime = runtime(
            PermissionConfig {
                ask: vec!["Bash(npm publish *)".to_string()],
                ..Default::default()
            },
            PermissionMode::DontAsk,
        );
        let output = runtime
            .authorize(
                "call-1",
                "bash",
                &json!({"command": "npm publish --dry-run"}),
            )
            .await
            .expect_err("dontAsk should deny explicit ask rules");
        assert!(output.is_error);
        assert_eq!(output.json["permission"]["decision"], "denied");
        assert!(
            output.json["permission"]["reason"]
                .as_str()
                .unwrap_or_default()
                .contains("dontAsk")
        );
    }
}
