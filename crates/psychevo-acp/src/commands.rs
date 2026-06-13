#[allow(unused_imports)]
pub(crate) use super::*;
impl PsychevoAcpAgent {
    pub(crate) async fn request_command_approval(
        &self,
        session_id: &SessionId,
        cx: &ConnectionTo<Client>,
        command: &str,
        reason: &str,
    ) -> bool {
        let tool_call = ToolCallUpdate::new(
            format!("slash_command_{}", Uuid::now_v7()),
            ToolCallUpdateFields::new()
                .title(format!("Command: {command}"))
                .status(ToolCallStatus::Pending)
                .raw_input(json!({
                    "command": command,
                    "reason": reason,
                })),
        );
        let options = vec![
            PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
            PermissionOption::new("deny", "Deny", PermissionOptionKind::RejectOnce),
        ];
        match cx
            .send_request(RequestPermissionRequest::new(
                session_id.clone(),
                tool_call,
                options,
            ))
            .block_task()
            .await
        {
            Ok(response) => matches!(
                response.outcome,
                RequestPermissionOutcome::Selected(selected)
                    if selected.option_id.to_string() == "allow_once"
            ),
            Err(_) => false,
        }
    }

    pub(crate) async fn skills_command_text(
        &self,
        session_id: &SessionId,
        session: &AcpSession,
        args: Option<&str>,
        cx: &ConnectionTo<Client>,
    ) -> Result<String, Error> {
        let Some(args) = args.map(str::trim).filter(|value| !value.is_empty()) else {
            return self.skills_dashboard_text(session);
        };
        let mut parts = args.split_whitespace().collect::<Vec<_>>();
        let action = parts.remove(0).to_ascii_lowercase();
        match action.as_str() {
            "help" | "--help" | "-h" => self.skills_dashboard_text(session),
            "list" => self.skills_list_text(session, None),
            "browse" | "search" => self.skills_list_text(session, Some(&parts.join(" "))),
            "inspect" => self.skills_inspect_text(session, parts.first().copied()),
            "check" => Ok(self.skills_check_text(session)),
            "audit" => self.skills_audit_text(session, &parts),
            "reload" => Ok(self.skills_reload_text(session)),
            "install" | "uninstall" | "config" => {
                if !self
                    .request_command_approval(session_id, cx, "/skills", "change local skill state")
                    .await
                {
                    return Ok("permission denied".to_string());
                }
                self.skills_mutation_text(session, action.as_str(), &parts)
            }
            _ => Ok(format!(
                "unknown /skills action: {action}\nSupported: list, browse, search, inspect, check, audit, reload"
            )),
        }
    }

    pub(crate) fn skills_dashboard_text(&self, session: &AcpSession) -> Result<String, Error> {
        let catalog = self.skill_catalog(session)?;
        let bundles = list_skill_bundles(&self.options.home, &session.cwd).unwrap_or_default();
        Ok([
            "Skills hub".to_string(),
            format!(
                "installed: {} skills, {} bundles",
                catalog.skills.len(),
                bundles.len()
            ),
            "/skills list - list installed skills".to_string(),
            "/skills search <query> - search installed skills".to_string(),
            "/skills inspect <name> - show local skill metadata".to_string(),
            "/skills check - check configured hub updates".to_string(),
            "/skills audit [name] - scan local skills".to_string(),
            "/skills reload - refresh skill context".to_string(),
            "/skills install <identifier-or-path> [--scope global|project] [--name <name>]"
                .to_string(),
            "/skills uninstall <name>".to_string(),
            "/skills config enable|disable <name> [--scope global|project]".to_string(),
        ]
        .join("\n"))
    }

    pub(crate) fn skills_list_text(
        &self,
        session: &AcpSession,
        query: Option<&str>,
    ) -> Result<String, Error> {
        let catalog = self.skill_catalog(session)?;
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let mut rows = catalog
            .skills
            .iter()
            .filter(|skill| {
                query.is_none_or(|query| {
                    let query = query.to_ascii_lowercase();
                    skill.name.to_ascii_lowercase().contains(&query)
                        || skill.description.to_ascii_lowercase().contains(&query)
                })
            })
            .map(|skill| format!("{}: {}", skill.name, skill.description))
            .collect::<Vec<_>>();
        rows.sort();
        if rows.is_empty() {
            Ok("No skills found.".to_string())
        } else {
            Ok(rows.join("\n"))
        }
    }

    pub(crate) fn skills_inspect_text(
        &self,
        session: &AcpSession,
        name: Option<&str>,
    ) -> Result<String, Error> {
        let Some(name) = name else {
            return Ok("usage: /skills inspect <name>".to_string());
        };
        let catalog = self.skill_catalog(session)?;
        let Some(skill) = catalog.skills.iter().find(|skill| {
            skill.name == name
                || psychevo_runtime::command_registry::normalize_dynamic_skill_name(&skill.name)
                    == psychevo_runtime::command_registry::normalize_dynamic_skill_name(name)
        }) else {
            return Ok(format!("skill not found: {name}"));
        };
        Ok(format!(
            "{}\n{}\npath: {}",
            skill.name,
            skill.description,
            skill.file_path.display()
        ))
    }

    pub(crate) fn skills_check_text(&self, session: &AcpSession) -> String {
        let skill_count = self
            .skill_catalog(session)
            .map(|catalog| catalog.skills.len())
            .unwrap_or(0);
        let bundle_count = list_skill_bundles(&self.options.home, &session.cwd)
            .map(|bundles| bundles.len())
            .unwrap_or(0);
        format!(
            "no hub update source configured\ninstalled: {skill_count} skills, {bundle_count} bundles"
        )
    }

    pub(crate) fn skills_audit_text(
        &self,
        session: &AcpSession,
        args: &[&str],
    ) -> Result<String, Error> {
        let catalog = self.skill_catalog(session)?;
        if let Some(name) = args.first() {
            let normalized = psychevo_runtime::command_registry::normalize_dynamic_skill_name(name);
            let Some(skill) = catalog.skills.iter().find(|skill| {
                skill.name == *name
                    || psychevo_runtime::command_registry::normalize_dynamic_skill_name(&skill.name)
                        == normalized
            }) else {
                return Ok(format!("unknown skill: {name}"));
            };
            return scan_skill_path(&skill.base_dir)
                .map(|scan| {
                    format!(
                        "{}: {:?} ({} findings)",
                        skill.name,
                        scan.verdict,
                        scan.findings.len()
                    )
                })
                .map_err(acp_internal_error);
        }
        if catalog.skills.is_empty() {
            return Ok("No skills found.".to_string());
        }
        Ok(catalog
            .skills
            .iter()
            .map(|skill| match scan_skill_path(&skill.base_dir) {
                Ok(scan) => format!(
                    "{}: {:?} ({} findings)",
                    skill.name,
                    scan.verdict,
                    scan.findings.len()
                ),
                Err(err) => format!("{}: error: {err:#}", skill.name),
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub(crate) fn skills_reload_text(&self, session: &AcpSession) -> String {
        let skill_count = self
            .skill_catalog(session)
            .map(|catalog| catalog.skills.len())
            .unwrap_or(0);
        let bundle_count = list_skill_bundles(&self.options.home, &session.cwd)
            .map(|bundles| bundles.len())
            .unwrap_or(0);
        format!("reloaded skills: {skill_count} skills, {bundle_count} bundles")
    }

    pub(crate) fn skills_mutation_text(
        &self,
        session: &AcpSession,
        action: &str,
        args: &[&str],
    ) -> Result<String, Error> {
        match action {
            "install" => {
                let Some(source) = args.first() else {
                    return Ok("usage: /skills install <identifier-or-path> [--scope global|project] [--name <name>]".to_string());
                };
                let value = install_skill(
                    &self.options.home,
                    &session.cwd,
                    InstallOptions {
                        source: (*source).to_string(),
                        target: skill_scope_from_args(args),
                        name: skill_option_value(args, "--name").map(ToOwned::to_owned),
                        all: args.contains(&"--all"),
                        force: args.contains(&"--force"),
                    },
                )
                .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            "uninstall" => {
                let Some(name) = args.first() else {
                    return Ok("usage: /skills uninstall <name>".to_string());
                };
                let catalog = self.skill_catalog(session)?;
                let value = remove_skill(&catalog, &self.options.home, &session.cwd, name)
                    .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            "config" => self.skills_config_mutation_text(session, args),
            _ => Ok("unsupported skill mutation".to_string()),
        }
    }

    pub(crate) fn skills_config_mutation_text(
        &self,
        session: &AcpSession,
        args: &[&str],
    ) -> Result<String, Error> {
        let Some(action) = args.first() else {
            return Ok("usage: /skills config enable|disable|set ...".to_string());
        };
        match *action {
            "enable" | "disable" => {
                let Some(name) = args.get(1) else {
                    return Ok(format!(
                        "usage: /skills config {action} <name> [--scope global|project]"
                    ));
                };
                let value = set_skill_enabled(
                    &self.options.home,
                    &session.cwd,
                    skill_scope_from_args(args),
                    name,
                    *action == "enable",
                )
                .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            "set" => {
                let filtered = skill_args_without_scope(args);
                if filtered.len() < 3 {
                    return Ok("usage: /skills config set skills.config.<key> <value> [--scope global|project]".to_string());
                }
                let value = serde_json::from_str::<Value>(filtered[2])
                    .unwrap_or_else(|_| Value::String(filtered[2].to_string()));
                let value = set_skill_config_value(
                    &self.options.home,
                    &session.cwd,
                    skill_scope_from_args(args),
                    filtered[1],
                    value,
                )
                .map_err(acp_internal_error)?;
                serde_json::to_string_pretty(&value).map_err(acp_internal_error)
            }
            other => Ok(format!("unknown /skills config action: {other}")),
        }
    }

    pub(crate) fn skill_catalog(
        &self,
        session: &AcpSession,
    ) -> Result<psychevo_runtime::SkillCatalog, Error> {
        discover_skills(&SkillDiscoveryOptions {
            home: self.options.home.clone(),
            workdir: session.cwd.clone(),
            config_path: self.options.config_path.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            no_skills: false,
        })
        .map_err(acp_internal_error)
    }

    pub(crate) fn bundles_command_text(
        &self,
        session: &AcpSession,
        args: Option<&str>,
    ) -> Result<String, Error> {
        match args.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("list") => {
                let bundles = list_skill_bundles(&self.options.home, &session.cwd)
                    .map_err(acp_internal_error)?;
                if bundles.is_empty() {
                    return Ok("No skill bundles found.".to_string());
                }
                Ok(bundles
                    .into_iter()
                    .map(|bundle| {
                        format!(
                            "{}: {} [{}]",
                            bundle.slug,
                            bundle.description,
                            bundle.skills.join(", ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"))
            }
            Some(_) => Ok("Supported bundle commands: /bundles, /bundles list".to_string()),
        }
    }

    pub(crate) fn curator_command_text(&self, args: Option<&str>) -> String {
        match args.map(str::trim).filter(|value| !value.is_empty()) {
            None | Some("status") => [
                "Skill curator",
                "status: enabled",
                "scope: global",
                "automatic destructive actions: disabled",
            ]
            .join("\n"),
            Some(_) => "Supported curator commands: /curator, /curator status".to_string(),
        }
    }

    pub(crate) fn write_artifact_text(
        &self,
        session: &AcpSession,
        artifact_kind: SessionArtifactKind,
        args: Option<&str>,
    ) -> Result<String, Error> {
        let Some(runtime_session_id) = session.runtime_session_id.as_deref() else {
            return Ok("no runtime session yet".to_string());
        };
        let parsed = parse_artifact_args(args.unwrap_or(""), artifact_kind)
            .map_err(|message| Error::invalid_params().data(message))?;
        let format = parsed.format.unwrap_or(SessionExportFormat::Markdown);
        let include = parsed
            .include
            .unwrap_or_else(|| SessionExportIncludeSet::default_for(artifact_kind));
        let path = parsed.path.unwrap_or_else(|| {
            session.cwd.join(default_session_export_filename(
                runtime_session_id,
                format,
                artifact_kind,
            ))
        });
        let path = if path.is_absolute() {
            path
        } else {
            session.cwd.join(path)
        };
        let store = self.state.store().clone();
        let result = psychevo_runtime::write_session_export(
            &store,
            runtime_session_id,
            &path,
            SessionExportOptions {
                format,
                include,
                artifact_kind,
            },
        )
        .map_err(acp_internal_error)?;
        Ok(format!(
            "{}: {} ({} bytes)",
            artifact_kind.as_str(),
            result.path.display(),
            result.bytes
        ))
    }

    pub(crate) fn auth_methods(&self, terminal_auth: bool) -> Vec<AuthMethod> {
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let options = self.probe_run_options(cwd, None);
        let selected_provider = selected_configured_model(&options)
            .ok()
            .flatten()
            .map(|model| model.provider);
        let providers = model_catalog_providers(&options).unwrap_or_default();
        let mut methods = Vec::new();
        if let Some(provider_id) = selected_provider
            && let Some(provider) = providers
                .iter()
                .find(|provider| provider.provider == provider_id && provider.fetchable())
        {
            methods.push(AuthMethod::Agent(
                AuthMethodAgent::new(
                    provider.provider.clone(),
                    format!("{} credentials", provider.display_label),
                )
                .description(format!(
                    "Use configured credentials for {}.",
                    provider.display_label
                )),
            ));
        }
        if terminal_auth {
            methods.push(AuthMethod::Terminal(
                AuthMethodTerminal::new(TERMINAL_SETUP_AUTH_METHOD_ID, "Psychevo setup")
                    .description("Configure Psychevo provider credentials in a terminal.")
                    .args(vec!["--setup".to_string()]),
            ));
        }
        methods
    }
}

pub(crate) enum SlashPromptAction {
    NotSlashOrPassThrough,
    Handled(PromptResponse),
    RunPrompt(String),
}

pub(crate) const TERMINAL_SETUP_AUTH_METHOD_ID: &str = "psychevo-setup";
pub(crate) const ACP_COMMAND_ADVERTISEMENT_LIMIT: usize = 100;

pub(crate) fn acp_command_capabilities()
-> &'static [psychevo_runtime::command_registry::CommandCapability] {
    use psychevo_runtime::command_registry::CommandCapability;
    &[
        CommandCapability::ActiveTurnControl,
        CommandCapability::Queue,
        CommandCapability::SessionSwitch,
        CommandCapability::SessionRevert,
        CommandCapability::ArtifactWrite,
        CommandCapability::WorkspaceDiff,
        CommandCapability::ConfigWrite,
        CommandCapability::PolicyWrite,
        CommandCapability::SkillStateWrite,
    ]
}

pub(crate) fn send_slash_text(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    text: impl Into<String>,
) -> SlashPromptAction {
    send_session_update(
        cx,
        session_id.clone(),
        SessionUpdate::AgentMessageChunk(ContentChunk::new(text.into().into())),
    );
    SlashPromptAction::Handled(PromptResponse::new(StopReason::EndTurn))
}

pub(crate) fn send_diff_tool_call(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    diff: &WorkspaceDiff,
) -> SlashPromptAction {
    let call_id = format!("slash_diff_{}", Uuid::now_v7());
    let (start, completed) = diff_tool_call_updates(call_id, diff);
    send_session_update(cx, session_id.clone(), start);
    send_session_update(cx, session_id.clone(), completed);
    SlashPromptAction::Handled(PromptResponse::new(StopReason::EndTurn))
}

fn diff_tool_call_updates(
    call_id: impl Into<String>,
    diff: &WorkspaceDiff,
) -> (SessionUpdate, SessionUpdate) {
    let call_id = call_id.into();
    (
        SessionUpdate::ToolCall(
            ToolCall::new(call_id.clone(), "Workspace diff")
                .kind(ToolKind::Read)
                .status(ToolCallStatus::InProgress)
                .raw_input(json!({ "command": "/diff" })),
        ),
        SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
            call_id,
            ToolCallUpdateFields::new()
                .title("Workspace diff")
                .kind(ToolKind::Read)
                .status(ToolCallStatus::Completed)
                .content(acp_diff_content(diff))
                .raw_output(diff_raw_output(diff)),
        )),
    )
}

fn acp_diff_content(diff: &WorkspaceDiff) -> Vec<ToolCallContent> {
    diff.files
        .iter()
        .map(|file| {
            let new_text = file.new_text.clone().unwrap_or_else(|| {
                file.placeholder
                    .clone()
                    .unwrap_or_else(|| format!("diff unavailable for {}", file.path))
            });
            let mut acp_diff = AcpDiff::new(PathBuf::from(&file.path), new_text);
            if let Some(old_text) = file.old_text.clone() {
                acp_diff = acp_diff.old_text(old_text);
            }
            ToolCallContent::Diff(acp_diff)
        })
        .collect()
}

fn diff_raw_output(diff: &WorkspaceDiff) -> Value {
    let status = if !diff.is_git_repo {
        "not_git_repo"
    } else if diff.is_empty() {
        "empty"
    } else {
        "ok"
    };
    json!({
        "status": status,
        "file_count": diff.files.len(),
        "truncation": diff.truncation,
        "files": diff.files.iter().map(diff_file_summary).collect::<Vec<_>>(),
    })
}

fn diff_file_summary(file: &WorkspaceDiffFile) -> Value {
    json!({
        "path": file.path,
        "status": file.status,
        "binary": file.binary,
        "unreadable": file.unreadable,
        "placeholder": file.placeholder,
    })
}

pub(crate) fn user_text_message(text: &str) -> Message {
    Message::User {
        content: vec![UserContentBlock::text(text)],
        timestamp_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as i64,
    }
}

pub(crate) fn resolve_session_reference(
    reference: &str,
    sessions: &[SessionSummary],
) -> Option<SessionSummary> {
    if sessions.is_empty() {
        return None;
    }
    if reference.is_empty() || reference == "latest" {
        return sessions.first().cloned();
    }
    if let Ok(index) = reference.parse::<usize>()
        && index > 0
    {
        return sessions.get(index - 1).cloned();
    }
    let id_matches = sessions
        .iter()
        .filter(|summary| summary.id.starts_with(reference))
        .cloned()
        .collect::<Vec<_>>();
    if id_matches.len() == 1 {
        return id_matches.into_iter().next();
    }
    let title_matches = sessions
        .iter()
        .filter(|summary| summary.title.as_deref() == Some(reference))
        .cloned()
        .collect::<Vec<_>>();
    (title_matches.len() == 1)
        .then(|| title_matches.into_iter().next())
        .flatten()
}

pub(crate) fn ambiguous_session_matches(
    reference: &str,
    sessions: &[SessionSummary],
) -> Vec<SessionSummary> {
    if reference.is_empty() || reference == "latest" {
        return Vec::new();
    }
    let id_matches = sessions
        .iter()
        .filter(|summary| summary.id.starts_with(reference))
        .cloned()
        .collect::<Vec<_>>();
    if id_matches.len() > 1 {
        return id_matches;
    }
    let title_matches = sessions
        .iter()
        .filter(|summary| summary.title.as_deref() == Some(reference))
        .cloned()
        .collect::<Vec<_>>();
    if title_matches.len() > 1 {
        title_matches
    } else {
        Vec::new()
    }
}

pub(crate) fn reasoning_effort_value(value: &str) -> Option<String> {
    (value != "none").then(|| value.to_string())
}

pub(crate) fn available_commands_from(
    available: psychevo_runtime::command_registry::AvailableSlashCommands,
) -> Vec<AvailableCommand> {
    available
        .commands
        .into_iter()
        .map(|command| {
            let description = if command.aliases.is_empty() {
                command.summary
            } else {
                format!(
                    "{} (aliases: {})",
                    command.summary,
                    command.aliases.join(", ")
                )
            };
            let input = match command.argument_kind {
                psychevo_runtime::command_registry::CommandArgumentKind::None => None,
                _ => Some(AvailableCommandInput::Unstructured(
                    UnstructuredCommandInput::new(command.usage),
                )),
            };
            AvailableCommand::new(command.name, description).input(input)
        })
        .collect()
}

pub(crate) fn available_command_lines_from(commands: Vec<AvailableCommand>) -> Vec<String> {
    commands
        .into_iter()
        .map(|command| {
            let input_hint = command
                .input
                .as_ref()
                .map(|input| match input {
                    AvailableCommandInput::Unstructured(input) => input.hint.clone(),
                    _ => String::new(),
                })
                .unwrap_or_default();
            let display = if input_hint.starts_with('/') {
                input_hint
            } else if input_hint.is_empty() {
                format!("/{}", command.name)
            } else {
                format!("/{} {}", command.name, input_hint)
            };
            format!("- {display} - {}", command.description)
        })
        .collect()
}

pub(crate) struct ParsedArtifactArgs {
    pub(crate) path: Option<PathBuf>,
    pub(crate) format: Option<SessionExportFormat>,
    pub(crate) include: Option<SessionExportIncludeSet>,
}

pub(crate) fn parse_artifact_args(
    args: &str,
    artifact_kind: SessionArtifactKind,
) -> std::result::Result<ParsedArtifactArgs, String> {
    let tokens = args.split_whitespace().collect::<Vec<_>>();
    let mut path = None;
    let mut format = None;
    let mut include = None;
    let mut index = 0usize;
    while index < tokens.len() {
        match tokens[index] {
            "--format" | "-f" if artifact_kind == SessionArtifactKind::Export => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err(
                        "usage: /export [path] [-f|--format markdown|json] [-i|--include list]"
                            .to_string(),
                    );
                };
                format = Some(parse_export_format(value)?);
            }
            value
                if artifact_kind == SessionArtifactKind::Export
                    && value.starts_with("--format=") =>
            {
                format = Some(parse_export_format(value.trim_start_matches("--format="))?);
            }
            "--include" | "-i" => {
                index += 1;
                let Some(value) = tokens.get(index) else {
                    return Err("usage: /export|/share [path] [-i|--include list]".to_string());
                };
                include = Some(
                    SessionExportIncludeSet::parse(value, artifact_kind)
                        .map_err(|err| err.to_string())?,
                );
            }
            value if value.starts_with("--include=") => {
                include = Some(
                    SessionExportIncludeSet::parse(
                        value.trim_start_matches("--include="),
                        artifact_kind,
                    )
                    .map_err(|err| err.to_string())?,
                );
            }
            value if value.starts_with('-') => {
                return Err(format!("unsupported option: {value}"));
            }
            value => {
                if path.is_some() {
                    return Err("only one output path is supported".to_string());
                }
                path = Some(PathBuf::from(value));
            }
        }
        index += 1;
    }
    Ok(ParsedArtifactArgs {
        path,
        format,
        include,
    })
}

pub(crate) fn parse_export_format(value: &str) -> std::result::Result<SessionExportFormat, String> {
    match value {
        "markdown" | "md" => Ok(SessionExportFormat::Markdown),
        "json" => Ok(SessionExportFormat::Json),
        _ => Err("format must be markdown or json".to_string()),
    }
}

pub(crate) fn skill_scope_from_args(args: &[&str]) -> SkillTarget {
    match skill_option_value(args, "--scope") {
        Some("project") | Some("local") => SkillTarget::Project,
        _ => SkillTarget::Global,
    }
}

pub(crate) fn skill_option_value<'a>(args: &'a [&str], option: &str) -> Option<&'a str> {
    args.windows(2)
        .find_map(|window| (window[0] == option).then_some(window[1]))
}

pub(crate) fn skill_args_without_scope<'a>(args: &'a [&str]) -> Vec<&'a str> {
    let mut filtered = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if *arg == "--scope" {
            skip_next = true;
            continue;
        }
        filtered.push(*arg);
    }
    filtered
}

#[derive(Clone)]
pub(crate) struct AcpApprovalHandler {
    pub(crate) session_id: SessionId,
    pub(crate) cx: ConnectionTo<Client>,
}

impl fmt::Debug for AcpApprovalHandler {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("AcpApprovalHandler")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl ApprovalHandler for AcpApprovalHandler {
    fn request_permission(
        &self,
        request: PermissionApprovalRequest,
    ) -> BoxFuture<'static, PermissionApprovalDecision> {
        let session_id = self.session_id.clone();
        let cx = self.cx.clone();
        Box::pin(async move {
            let tool_call = ToolCallUpdate::new(
                request.tool_call_id.clone(),
                ToolCallUpdateFields::new()
                    .title(format!("Permission: {}", request.tool_name))
                    .status(ToolCallStatus::Pending)
                    .raw_input(json!({
                        "summary": request.summary,
                        "reason": request.reason,
                        "matched_rule": request.matched_rule,
                        "suggested_rule": request.suggested_rule,
                    })),
            );
            let mut options = vec![
                PermissionOption::new("allow_once", "Allow once", PermissionOptionKind::AllowOnce),
                PermissionOption::new(
                    "allow_session",
                    "Allow for session",
                    PermissionOptionKind::AllowAlways,
                ),
                PermissionOption::new("deny", "Deny", PermissionOptionKind::RejectOnce),
            ];
            if request.allow_always {
                options.insert(
                    2,
                    PermissionOption::new(
                        "allow_always",
                        "Allow always",
                        PermissionOptionKind::AllowAlways,
                    ),
                );
            }
            match cx
                .send_request(RequestPermissionRequest::new(
                    session_id, tool_call, options,
                ))
                .block_task()
                .await
            {
                Ok(response) => match response.outcome {
                    RequestPermissionOutcome::Cancelled => PermissionApprovalDecision::deny(),
                    RequestPermissionOutcome::Selected(selected) => {
                        match selected.option_id.to_string().as_str() {
                            "allow_once" => PermissionApprovalDecision::allow_once(),
                            "allow_session" => PermissionApprovalDecision::allow_session(),
                            "allow_always" => PermissionApprovalDecision::allow_always(),
                            _ => PermissionApprovalDecision::deny(),
                        }
                    }
                    _ => PermissionApprovalDecision::deny(),
                },
                Err(_) => PermissionApprovalDecision::deny(),
            }
        })
    }
}

pub(crate) fn send_session_setup_updates(
    cx: &ConnectionTo<Client>,
    session_id: SessionId,
    mode: RunMode,
    commands: Vec<AvailableCommand>,
) {
    send_session_update(
        cx,
        session_id.clone(),
        SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new(mode.as_str())),
    );
    send_session_update(
        cx,
        session_id.clone(),
        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(session_config_options(mode))),
    );
    send_session_update(
        cx,
        session_id,
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands)),
    );
}

pub(crate) fn send_session_update(
    cx: &ConnectionTo<Client>,
    session_id: SessionId,
    update: SessionUpdate,
) {
    let _ = cx.send_notification(SessionNotification::new(session_id, update));
}

#[derive(Debug, Default)]
pub(crate) struct AcpLiveProjection {
    reasoning_offsets: HashMap<String, usize>,
    terminal_output: bool,
    terminal_offsets: HashMap<String, usize>,
}

impl AcpLiveProjection {
    pub(crate) fn new(terminal_output: bool) -> Self {
        Self {
            terminal_output,
            ..Self::default()
        }
    }
}

pub(crate) fn send_gateway_event_update(
    cx: &ConnectionTo<Client>,
    session_id: &SessionId,
    event: GatewayEvent,
    projection: &mut AcpLiveProjection,
) {
    match event {
        GatewayEvent::EntryDelta { delta, .. } => send_session_update(
            cx,
            session_id.clone(),
            SessionUpdate::AgentThoughtChunk(ContentChunk::new(delta.into())),
        ),
        GatewayEvent::EntryStarted { entry, .. }
        | GatewayEvent::EntryUpdated { entry, .. }
        | GatewayEvent::EntryCompleted { entry, .. } => {
            for update in transcript_entry_session_updates(&entry, projection, true) {
                send_session_update(cx, session_id.clone(), update);
            }
        }
        GatewayEvent::Warning { message, .. } => send_session_update(
            cx,
            session_id.clone(),
            SessionUpdate::AgentMessageChunk(ContentChunk::new(
                format!("warning: {message}").into(),
            )),
        ),
        GatewayEvent::TurnCompleted {
            committed_entries, ..
        } => {
            for entry in committed_entries {
                for update in transcript_entry_session_updates(&entry, projection, false) {
                    send_session_update(cx, session_id.clone(), update);
                }
            }
        }
        GatewayEvent::TurnStarted { .. }
        | GatewayEvent::TurnQueued { .. }
        | GatewayEvent::PermissionRequested { .. }
        | GatewayEvent::PermissionResolved { .. }
        | GatewayEvent::ClarifyRequested { .. }
        | GatewayEvent::ClarifyResolved { .. } => {}
    }
}

fn transcript_entry_session_updates(
    entry: &TranscriptEntry,
    projection: &mut AcpLiveProjection,
    include_reasoning: bool,
) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();
    for block in &entry.blocks {
        if include_reasoning
            && block.kind == TranscriptBlockKind::Reasoning
            && let Some(delta) = reasoning_block_delta(block, projection)
        {
            updates.push(SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                delta.into(),
            )));
        }
        if let Some(update) = transcript_block_session_update(block, projection, include_reasoning)
        {
            updates.push(update);
        }
    }
    updates
}

fn transcript_block_session_update(
    block: &TranscriptBlock,
    projection: &mut AcpLiveProjection,
    live_presentation: bool,
) -> Option<SessionUpdate> {
    if !matches!(
        block.kind,
        TranscriptBlockKind::Tool
            | TranscriptBlockKind::ToolCall
            | TranscriptBlockKind::ToolResult
            | TranscriptBlockKind::Shell
            | TranscriptBlockKind::File
            | TranscriptBlockKind::Web
            | TranscriptBlockKind::Mcp
            | TranscriptBlockKind::Clarify
            | TranscriptBlockKind::Diff
            | TranscriptBlockKind::Artifact
    ) {
        return None;
    }
    let call_id = transcript_tool_call_id(block);
    let tool_name = block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_name"))
        .and_then(Value::as_str)
        .or(block.title.as_deref())
        .unwrap_or("tool");
    let use_terminal_output =
        live_presentation && projection.terminal_output && tool_name == "exec_command";
    let content = transcript_tool_content(block, tool_name, &call_id, use_terminal_output);
    let mut update = ToolCallUpdate::new(
        call_id,
        ToolCallUpdateFields::new()
            .title(transcript_tool_title(block, tool_name))
            .kind(tool_kind(tool_name))
            .status(transcript_tool_status(block.status))
            .content(content)
            .raw_input(block.metadata.clone()),
    );
    if use_terminal_output
        && let Some(meta) = terminal_output_meta(block, update.tool_call_id.0.as_ref(), projection)
    {
        update = update.meta(meta);
    }
    Some(SessionUpdate::ToolCallUpdate(update))
}

fn reasoning_block_delta(
    block: &TranscriptBlock,
    projection: &mut AcpLiveProjection,
) -> Option<String> {
    let text = transcript_block_text(block)?.to_string();
    if text.trim().is_empty() {
        return None;
    }
    let offset = projection
        .reasoning_offsets
        .entry(block.id.clone())
        .or_insert(0);
    if *offset > text.len() {
        *offset = 0;
    }
    let delta = text.get(*offset..)?.to_string();
    *offset = text.len();
    if delta.is_empty() { None } else { Some(delta) }
}

fn transcript_tool_title(block: &TranscriptBlock, tool_name: &str) -> String {
    if let Some(title) = block
        .title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        return title.to_string();
    }
    if tool_name == "exec_command"
        && let Some(command) =
            exec_command_arg(block.metadata.as_ref()).and_then(first_shell_command_line)
    {
        return format!("exec_command {command}");
    }
    tool_title(tool_name)
}

fn transcript_tool_content(
    block: &TranscriptBlock,
    tool_name: &str,
    call_id: &str,
    use_terminal_output: bool,
) -> Vec<ToolCallContent> {
    if tool_name == "exec_command"
        && let Some(command) = exec_command_arg(block.metadata.as_ref())
    {
        let command_text = format!("$ {command}");
        if use_terminal_output {
            return vec![
                ToolCallContent::from(command_text),
                ToolCallContent::Terminal(Terminal::new(call_id.to_string())),
            ];
        }
        let mut text = command_text;
        if let Some(output) = transcript_block_text(block).filter(|value| !value.trim().is_empty())
        {
            text.push_str("\n\n");
            text.push_str(output);
        }
        return vec![ToolCallContent::from(text)];
    }
    transcript_block_text(block)
        .filter(|text| !text.trim().is_empty())
        .map(|text| vec![ToolCallContent::from(text.to_string())])
        .unwrap_or_default()
}

fn transcript_block_text(block: &TranscriptBlock) -> Option<&str> {
    block
        .result
        .as_ref()
        .map(|result| result.content.as_str())
        .or(block
            .detail
            .as_deref()
            .or(block.body.as_deref())
            .or(block.preview.as_deref()))
}

fn exec_command_arg(metadata: Option<&Value>) -> Option<&str> {
    metadata?
        .get("args")
        .and_then(|args| args.get("cmd"))
        .and_then(Value::as_str)
}

fn first_shell_command_line(text: &str) -> Option<&str> {
    let mut first_non_empty = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        first_non_empty.get_or_insert(line);
        if !line.starts_with('#') {
            return Some(line);
        }
    }
    first_non_empty
}

fn terminal_output_meta(
    block: &TranscriptBlock,
    call_id: &str,
    projection: &mut AcpLiveProjection,
) -> Option<Meta> {
    let command = exec_command_arg(block.metadata.as_ref())?;
    let first_update = !projection.terminal_offsets.contains_key(call_id);
    let mut meta = Meta::new();
    if first_update {
        meta.insert(
            "terminal_info".to_string(),
            json!({
                "terminal_id": call_id,
                "command": command,
            }),
        );
    }

    let output = transcript_block_text(block).unwrap_or_default();
    let offset = projection
        .terminal_offsets
        .entry(call_id.to_string())
        .or_insert(0);
    if *offset > output.len() {
        *offset = 0;
    }
    let mut data = String::new();
    if first_update {
        data.push_str("$ ");
        data.push_str(command);
        data.push('\n');
    }
    if let Some(delta) = output.get(*offset..) {
        data.push_str(delta);
    }
    *offset = output.len();
    if !data.is_empty() {
        meta.insert(
            "terminal_output".to_string(),
            json!({
                "terminal_id": call_id,
                "data": data,
            }),
        );
    }
    if matches!(
        block.status,
        TranscriptBlockStatus::Completed
            | TranscriptBlockStatus::Failed
            | TranscriptBlockStatus::Cancelled
    ) {
        meta.insert(
            "terminal_exit".to_string(),
            json!({
                "terminal_id": call_id,
                "exit_code": exec_exit_code(block.metadata.as_ref()),
                "signal": null,
            }),
        );
    }
    if meta.is_empty() { None } else { Some(meta) }
}

fn exec_exit_code(metadata: Option<&Value>) -> Option<i64> {
    metadata?
        .get("result")
        .and_then(|result| result.get("exit_code"))
        .and_then(Value::as_i64)
}

fn transcript_tool_call_id(block: &TranscriptBlock) -> String {
    block
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("tool_call_id"))
        .and_then(Value::as_str)
        .map(ToString::to_string)
        .unwrap_or_else(|| {
            block
                .id
                .rsplit_once("tool:")
                .map(|(_, id)| id)
                .unwrap_or(block.id.as_str())
                .to_string()
        })
}

fn transcript_tool_status(status: TranscriptBlockStatus) -> ToolCallStatus {
    match status {
        TranscriptBlockStatus::Pending => ToolCallStatus::Pending,
        TranscriptBlockStatus::Running => ToolCallStatus::InProgress,
        TranscriptBlockStatus::Completed | TranscriptBlockStatus::Info => ToolCallStatus::Completed,
        TranscriptBlockStatus::Failed | TranscriptBlockStatus::Cancelled => ToolCallStatus::Failed,
        TranscriptBlockStatus::NeedsInput => ToolCallStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use psychevo_gateway::TranscriptEntryRole;
    use psychevo_runtime::command_registry::{
        SlashCommandEffect, SlashCommandParse, SlashCommandSurface,
        available_slash_commands_for_surface, parse_slash_command_line, slash_invocation_effect,
    };
    use psychevo_runtime::{
        SessionExportInclude, WorkspaceDiffFileStatus, WorkspaceDiffTruncation,
    };

    #[test]
    fn acp_advertises_diff_and_allows_it_during_active_turns() {
        let available = available_slash_commands_for_surface(
            acp_command_capabilities(),
            true,
            &[],
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        );
        assert!(
            available
                .commands
                .iter()
                .any(|command| command.name == "diff"),
            "{available:?}"
        );

        let SlashCommandParse::Known(invocation) = parse_slash_command_line("/diff") else {
            panic!("expected /diff to parse");
        };
        let effect = slash_invocation_effect(
            &invocation,
            acp_command_capabilities(),
            SlashCommandSurface::Acp,
            true,
        )
        .expect("slash effect");
        assert_eq!(effect, SlashCommandEffect::Diff);
    }

    #[test]
    fn acp_advertises_undo_redo_when_idle() {
        let available = available_slash_commands_for_surface(
            acp_command_capabilities(),
            false,
            &[],
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        );
        let names = available
            .commands
            .iter()
            .map(|command| command.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"undo"), "{names:?}");
        assert!(names.contains(&"redo"), "{names:?}");

        let SlashCommandParse::Known(undo) = parse_slash_command_line("/undo") else {
            panic!("expected /undo to parse");
        };
        let effect = slash_invocation_effect(
            &undo,
            acp_command_capabilities(),
            SlashCommandSurface::Acp,
            false,
        )
        .expect("undo effect");
        assert_eq!(effect, SlashCommandEffect::Undo);

        let active = available_slash_commands_for_surface(
            acp_command_capabilities(),
            true,
            &[],
            ACP_COMMAND_ADVERTISEMENT_LIMIT,
        );
        assert!(!active.commands.iter().any(|command| command.name == "undo"));
        assert!(!active.commands.iter().any(|command| command.name == "redo"));
    }

    #[test]
    fn acp_export_parses_last_provider_response_include() {
        let parsed = parse_artifact_args(
            "out.json -f json -i last-provider-response",
            SessionArtifactKind::Export,
        )
        .expect("export args");
        assert_eq!(parsed.format, Some(SessionExportFormat::Json));
        assert!(parsed.path.as_deref() == Some(Path::new("out.json")));
        assert!(parsed.include.is_some_and(|include| {
            include.contains(SessionExportInclude::LastProviderResponse)
        }));

        let share = parse_artifact_args(
            "share.md -i last-provider-response",
            SessionArtifactKind::Share,
        );
        assert!(share.is_err());
        assert!(
            parse_artifact_args("out.json -i last-raw-response", SessionArtifactKind::Export)
                .is_err()
        );
    }

    #[test]
    fn diff_tool_call_update_uses_structured_diff_without_text_fallback() {
        let diff = sample_workspace_diff();
        let (start, completed) = diff_tool_call_updates("slash_diff_test", &diff);

        match start {
            SessionUpdate::ToolCall(call) => {
                assert_eq!(call.title, "Workspace diff");
                assert_eq!(call.kind, ToolKind::Read);
                assert_eq!(call.status, ToolCallStatus::InProgress);
                assert_eq!(
                    call.raw_input
                        .as_ref()
                        .and_then(|value| value.get("command"))
                        .and_then(Value::as_str),
                    Some("/diff")
                );
                assert!(call.content.is_empty());
            }
            SessionUpdate::AgentMessageChunk(_) => panic!("diff must not use assistant text"),
            other => panic!("unexpected start update: {other:?}"),
        }

        match completed {
            SessionUpdate::ToolCallUpdate(update) => {
                assert_eq!(update.fields.title.as_deref(), Some("Workspace diff"));
                assert_eq!(update.fields.kind, Some(ToolKind::Read));
                assert_eq!(update.fields.status, Some(ToolCallStatus::Completed));
                let content = update.fields.content.expect("diff content");
                assert_eq!(content.len(), 1);
                match &content[0] {
                    ToolCallContent::Diff(diff) => {
                        assert_eq!(diff.path, PathBuf::from("src/lib.rs"));
                        assert_eq!(diff.old_text.as_deref(), Some("old body\n"));
                        assert_eq!(diff.new_text, "new body\n");
                    }
                    other => panic!("unexpected content: {other:?}"),
                }

                let raw = update.fields.raw_output.expect("raw output");
                assert_eq!(raw.get("status").and_then(Value::as_str), Some("ok"));
                assert_eq!(raw.get("file_count").and_then(Value::as_u64), Some(1));
                assert_eq!(
                    raw.pointer("/truncation/truncated")
                        .and_then(Value::as_bool),
                    Some(true)
                );
                let raw_text = serde_json::to_string(&raw).expect("raw output json");
                assert!(!raw_text.contains("UNIFIED_PATCH_BODY_SHOULD_NOT_APPEAR"));
                assert!(!raw_text.contains("new body"));
            }
            SessionUpdate::AgentMessageChunk(_) => panic!("diff must not use assistant text"),
            other => panic!("unexpected completed update: {other:?}"),
        }
    }

    #[test]
    fn reasoning_blocks_emit_incremental_thought_chunks() {
        let mut projection = AcpLiveProjection::new(false);
        let mut block = sample_transcript_block(
            "reasoning-1",
            TranscriptBlockKind::Reasoning,
            TranscriptBlockStatus::Running,
            Some("Thinking"),
            Some("first"),
            None,
        );
        let entry = sample_transcript_entry(vec![block.clone()]);

        let updates = transcript_entry_session_updates(&entry, &mut projection, true);
        assert_eq!(updates.len(), 1);
        assert_eq!(thought_text(&updates[0]), Some("first"));

        block.body = Some("first second".to_string());
        let entry = sample_transcript_entry(vec![block.clone()]);
        let updates = transcript_entry_session_updates(&entry, &mut projection, true);
        assert_eq!(updates.len(), 1);
        assert_eq!(thought_text(&updates[0]), Some(" second"));

        let updates = transcript_entry_session_updates(&entry, &mut projection, true);
        assert!(updates.is_empty(), "{updates:?}");
        let updates = transcript_entry_session_updates(&entry, &mut projection, false);
        assert!(updates.is_empty(), "{updates:?}");
    }

    #[test]
    fn exec_command_update_shows_command_title_content_and_raw_input() {
        let mut projection = AcpLiveProjection::new(false);
        let block = sample_transcript_block(
            "tool:call_exec",
            TranscriptBlockKind::Shell,
            TranscriptBlockStatus::Running,
            Some("exec_command cargo test"),
            Some("running tests\n"),
            Some(json!({
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "cargo test\n--workspace"}
            })),
        );

        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        assert_eq!(
            update.fields.title.as_deref(),
            Some("exec_command cargo test")
        );
        assert_eq!(update.fields.kind, Some(ToolKind::Execute));
        assert_eq!(update.fields.status, Some(ToolCallStatus::InProgress));
        assert_eq!(
            update
                .fields
                .raw_input
                .as_ref()
                .and_then(|value| value.pointer("/args/cmd"))
                .and_then(Value::as_str),
            Some("cargo test\n--workspace")
        );
        let content = update.fields.content.expect("tool content");
        assert_eq!(
            tool_content_text(&content[0]),
            Some("$ cargo test\n--workspace\n\nrunning tests\n")
        );
    }

    #[test]
    fn terminal_output_opt_in_uses_terminal_content_and_meta() {
        let mut projection = AcpLiveProjection::new(true);
        let mut block = sample_transcript_block(
            "tool:call_exec",
            TranscriptBlockKind::Shell,
            TranscriptBlockStatus::Running,
            Some("exec_command python fetch.py"),
            Some("first\n"),
            Some(json!({
                "tool_name": "exec_command",
                "tool_call_id": "call_exec",
                "args": {"cmd": "python fetch.py"}
            })),
        );

        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        let content = update.fields.content.expect("terminal content");
        assert_eq!(tool_content_text(&content[0]), Some("$ python fetch.py"));
        assert!(matches!(&content[1], ToolCallContent::Terminal(_)));
        let meta = update.meta.expect("terminal meta");
        assert_eq!(meta["terminal_info"]["terminal_id"], "call_exec");
        assert_eq!(
            meta["terminal_output"]["data"],
            "$ python fetch.py\nfirst\n"
        );

        block.body = Some("first\nsecond\n".to_string());
        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        let meta = update.meta.expect("terminal meta");
        assert_eq!(meta["terminal_output"]["data"], "second\n");

        block.status = TranscriptBlockStatus::Completed;
        block.body = Some("first\nsecond\nthird\n".to_string());
        block.metadata = Some(json!({
            "tool_name": "exec_command",
            "tool_call_id": "call_exec",
            "args": {"cmd": "python fetch.py"},
            "result": {"exit_code": 0}
        }));
        let update =
            transcript_block_session_update(&block, &mut projection, true).expect("tool update");
        let SessionUpdate::ToolCallUpdate(update) = update else {
            panic!("unexpected update: {update:?}");
        };
        let meta = update.meta.expect("terminal meta");
        assert_eq!(meta["terminal_output"]["data"], "third\n");
        assert_eq!(meta["terminal_exit"]["terminal_id"], "call_exec");
        assert_eq!(meta["terminal_exit"]["exit_code"].as_i64(), Some(0));
    }

    fn thought_text(update: &SessionUpdate) -> Option<&str> {
        let SessionUpdate::AgentThoughtChunk(chunk) = update else {
            return None;
        };
        match &chunk.content {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        }
    }

    fn tool_content_text(content: &ToolCallContent) -> Option<&str> {
        let ToolCallContent::Content(content) = content else {
            return None;
        };
        match &content.content {
            ContentBlock::Text(text) => Some(text.text.as_str()),
            _ => None,
        }
    }

    fn sample_transcript_entry(blocks: Vec<TranscriptBlock>) -> TranscriptEntry {
        TranscriptEntry {
            id: "entry-1".to_string(),
            thread_id: "thread-1".to_string(),
            turn_id: Some("turn-1".to_string()),
            message_seq: None,
            role: TranscriptEntryRole::Assistant,
            status: TranscriptBlockStatus::Running,
            source: "live".to_string(),
            blocks,
            metadata: None,
            usage: None,
            accounting: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    fn sample_transcript_block(
        id: &str,
        kind: TranscriptBlockKind,
        status: TranscriptBlockStatus,
        title: Option<&str>,
        body: Option<&str>,
        metadata: Option<Value>,
    ) -> TranscriptBlock {
        TranscriptBlock {
            id: id.to_string(),
            kind,
            status,
            order: 0,
            source: "live".to_string(),
            title: title.map(ToString::to_string),
            body: body.map(ToString::to_string),
            preview: None,
            detail: None,
            artifact_ids: Vec::new(),
            metadata,
            result: None,
            created_at_ms: 1,
            updated_at_ms: 1,
        }
    }

    fn sample_workspace_diff() -> WorkspaceDiff {
        WorkspaceDiff {
            is_git_repo: true,
            files: vec![WorkspaceDiffFile {
                path: "src/lib.rs".to_string(),
                status: WorkspaceDiffFileStatus::Modified,
                old_text: Some("old body\n".to_string()),
                new_text: Some("new body\n".to_string()),
                binary: false,
                unreadable: false,
                placeholder: None,
            }],
            unified_diff:
                "diff --git a/src/lib.rs b/src/lib.rs\n+UNIFIED_PATCH_BODY_SHOULD_NOT_APPEAR\n"
                    .to_string(),
            truncation: WorkspaceDiffTruncation {
                truncated: true,
                max_bytes: 256,
                max_lines: 3000,
                omitted_bytes: 64,
                omitted_lines: 2,
            },
        }
    }
}
