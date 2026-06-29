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
            cwd: session.cwd.clone(),
            config_path: self.options.config_path.clone(),
            env: self.options.inherited_env.clone(),
            explicit_inputs: Vec::new(),
            additional_roots: Vec::new(),
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
