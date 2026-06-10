#[allow(unused_imports)]
pub(crate) use super::*;
impl TuiApp {
    pub(crate) async fn handle_line(&mut self, line: &str) -> Result<bool> {
        if let Some(shell) = parse_shell_escape_input(line) {
            if let Err(err) = self.submit_shell_command(shell.command).await {
                self.had_error = true;
                eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
            }
            return Ok(false);
        }
        match self.classify_submitted_slash_input(line) {
            Ok(SubmittedSlashInput::Command(command)) => self.handle_command(command).await,
            Ok(SubmittedSlashInput::PassThroughPrompt(prompt)) => {
                if let Err(err) = self.submit_prompt(prompt).await {
                    self.had_error = true;
                    eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
                }
                Ok(false)
            }
            Ok(SubmittedSlashInput::NotSlash) => {
                if let Err(err) = self.submit_prompt(line.to_string()).await {
                    self.had_error = true;
                    eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
                }
                Ok(false)
            }
            Err(err) => {
                self.had_error = true;
                eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
                Ok(false)
            }
        }
    }

    pub(crate) async fn handle_command(&mut self, command: SlashCommand) -> Result<bool> {
        let result = match command {
            SlashCommand::Help => {
                println!("{}", self.help_status_text());
                Ok(())
            }
            SlashCommand::Quit => return Ok(true),
            SlashCommand::Status => self.show_status(),
            SlashCommand::New => {
                self.begin_new_session_draft();
                Ok(())
            }
            SlashCommand::Sessions => self.show_session_list(),
            SlashCommand::Usage => {
                println!("{}", self.stats_status_text()?);
                Ok(())
            }
            SlashCommand::Context => {
                let live = self.last_context_snapshot.clone();
                let snapshot = self.context_status_snapshot(live.as_ref())?;
                self.last_context_snapshot = Some(snapshot.clone());
                println!(
                    "{}",
                    format_context_snapshot_text_with_options(
                        &snapshot,
                        ContextFormatOptions {
                            heading: true,
                            bar_width: None,
                        },
                    )
                );
                Ok(())
            }
            SlashCommand::Diff => {
                let diff = collect_workspace_diff(&self.workdir)?;
                println!("{}", workspace_diff_plain_text(&diff));
                Ok(())
            }
            SlashCommand::Refresh => {
                let session = self
                    .current_session
                    .clone()
                    .ok_or_else(|| anyhow!("no session context yet"))?;
                let result = reload_session_context(ReloadContextOptions {
                    state: self.state_runtime.clone(),
                    session,
                    config_path: self.config_path.clone(),
                    mode: Some(self.current_mode),
                    inherited_env: Some(self.env_map.clone()),
                    agent: self.current_agent.clone(),
                    no_agents: self.no_agents,
                    no_skills: self.no_skills,
                    invalidation_reason: "manual_reload".to_string(),
                    notice: None,
                })?;
                println!(
                    "reloaded context: {} v{}; side cleanup deleted {}",
                    result.prefix_hash,
                    result.version,
                    self.state_runtime.delete_sessions_for_workdir_with_source(
                        &self.workdir,
                        TUI_SIDE_SESSION_SOURCE,
                    )?
                );
                Ok(())
            }
            SlashCommand::ReloadContextDeprecated => {
                println!(
                    "{}",
                    self.renderer.status(RELOAD_CONTEXT_DEPRECATED_MESSAGE)
                );
                Ok(())
            }
            SlashCommand::Btw(_) => Err(anyhow!("/btw is only available in fullscreen TUI")),
            SlashCommand::Steer(_) => Err(anyhow!("/steer requires a running fullscreen turn")),
            SlashCommand::Queue(message) => {
                return self.submit_prompt(message).await.map(|_| false);
            }
            SlashCommand::PendingCancel => {
                println!("{}", self.renderer.status("no pending input"));
                Ok(())
            }
            SlashCommand::ModelShowScoped { .. } => self.show_model(),
            SlashCommand::VariantSet(variant) => self.set_variant(variant),
            SlashCommand::ModeSet(mode) => self.set_mode(mode),
            SlashCommand::Permissions => {
                println!("{}", self.permissions_status_text()?);
                Ok(())
            }
            SlashCommand::Sandbox => {
                println!("{}", self.sandbox_status_text()?);
                Ok(())
            }
            SlashCommand::ThinkingToggle => self.toggle_thinking(),
            SlashCommand::ThinkingSet(enabled) => self.set_thinking(enabled),
            SlashCommand::RawToggle => self.toggle_raw(),
            SlashCommand::RawSet(enabled) => self.set_raw(enabled),
            SlashCommand::Copy => self.copy_latest_answer_markdown_scripted(),
            SlashCommand::Export(options) => self
                .write_tui_export(&options)
                .map(|result| println!("exported: {}", result.path.display())),
            SlashCommand::Share(options) => self
                .write_tui_share(&options)
                .map(|result| println!("share: {}", result.path.display())),
            SlashCommand::Image { .. } => {
                Err(anyhow!("/image is only available in fullscreen TUI"))
            }
            SlashCommand::Rename(title) => self.rename_session(title),
            SlashCommand::Undo => self.undo_session_print(),
            SlashCommand::Redo => self.redo_session_print(),
            SlashCommand::Skills(args) => {
                println!("{}", self.skills_command_text(args.as_deref()));
                Ok(())
            }
            SlashCommand::Tools => {
                println!("{}", self.toolsets_status_text()?);
                Ok(())
            }
            SlashCommand::Bundles(args) => {
                println!("{}", self.bundles_command_text(args.as_deref()));
                Ok(())
            }
            SlashCommand::Curator(args) => {
                println!("{}", self.curator_command_text(args.as_deref()));
                Ok(())
            }
            SlashCommand::Agents => {
                println!("{}", self.agents_status_text());
                Ok(())
            }
            SlashCommand::Fork(prompt) => {
                let prompt = fork_prompt_marker(&prompt);
                return self.submit_prompt(prompt).await.map(|_| false);
            }
            SlashCommand::Compact(instructions) => self.run_scripted_compaction(instructions).await,
            SlashCommand::SkillInvoke { name, args } => {
                let Some(prompt) = self.skill_or_bundle_marker(&name, &args) else {
                    return Err(anyhow!("unknown skill or bundle: {name}"));
                };
                return self.submit_prompt(prompt).await.map(|_| false);
            }
            SlashCommand::Upcoming(command) => {
                println!(
                    "{}",
                    self.renderer
                        .status(&format!("/{command} is upcoming; no session changes made"))
                );
                Ok(())
            }
        };
        if let Err(err) = result {
            self.had_error = true;
            eprintln!("{}", self.renderer.error(&format!("error: {err:#}")));
        }
        Ok(false)
    }

    pub(crate) fn image_submission_degrades_to_text(
        &self,
        prompt: &str,
        images: &[ImageInput],
    ) -> bool {
        let has_image = !images.is_empty();
        let _ = prompt;
        has_image
            && self.selected_model.as_ref().is_some_and(|model| {
                model_metadata_explicitly_disallows_image_input(&model.metadata)
            })
    }

    pub(crate) fn skills_status_text(&self) -> String {
        self.skills_rows(None, false)
    }

    pub(crate) fn skills_rows(&self, query: Option<&str>, include_source: bool) -> String {
        let Some(catalog) = self.current_skill_catalog() else {
            return "No skills found.".to_string();
        };
        let query = query.map(str::trim).filter(|value| !value.is_empty());
        let mut rows = catalog
            .skills
            .iter()
            .filter(|skill| {
                query.is_none_or(|query| {
                    let query = query.to_ascii_lowercase();
                    skill.name.to_ascii_lowercase().contains(&query)
                        || skill.description.to_ascii_lowercase().contains(&query)
                        || skill
                            .category
                            .as_deref()
                            .unwrap_or_default()
                            .to_ascii_lowercase()
                            .contains(&query)
                        || skill
                            .tags
                            .iter()
                            .any(|tag| tag.to_ascii_lowercase().contains(&query))
                })
            })
            .map(|skill| {
                if include_source {
                    format!(
                        "{}: {} ({})",
                        skill.name,
                        skill.description,
                        skill.source.as_str()
                    )
                } else {
                    format!("{}: {}", skill.name, skill.description)
                }
            })
            .collect::<Vec<_>>();
        if rows.is_empty() {
            return "No skills found.".to_string();
        }
        rows.sort();
        rows.join("\n")
    }

    pub(crate) fn skills_command_text(&self, args: Option<&str>) -> String {
        let Some(args) = args.map(str::trim).filter(|value| !value.is_empty()) else {
            return self.skills_dashboard_text();
        };
        let mut parts = args.split_whitespace().collect::<Vec<_>>();
        let action = parts.remove(0).to_ascii_lowercase();
        match action.as_str() {
            "help" | "--help" | "-h" => self.skills_dashboard_text(),
            "list" => self.skills_status_text(),
            "browse" => self.skills_rows(Some(&parts.join(" ")), true),
            "search" => {
                if parts.is_empty() {
                    "usage: /skills search <query>".to_string()
                } else {
                    self.skills_rows(Some(&parts.join(" ")), true)
                }
            }
            "inspect" => self.skills_inspect_text(&parts),
            "check" => self.skills_check_text(),
            "audit" => self.skills_audit_text(&parts),
            "reload" => self.skills_reload_text(),
            "install" | "update" | "uninstall" | "publish" | "config" => {
                self.skills_mutation_text(action.as_str(), &parts)
            }
            other => format!(
                "unknown /skills action: {other}\nSupported: list, browse, search, inspect, check, audit, reload"
            ),
        }
    }

    pub(crate) fn skills_dashboard_text(&self) -> String {
        let skill_count = self
            .current_skill_catalog()
            .map(|catalog| catalog.skills.len())
            .unwrap_or(0);
        let bundle_count = self.current_skill_bundles().len();
        [
            "Skills hub".to_string(),
            format!("installed: {skill_count} skills, {bundle_count} bundles"),
            "/skills list - list installed skills".to_string(),
            "/skills browse [query] - browse local hub entries".to_string(),
            "/skills search <query> - search installed and indexed skills".to_string(),
            "/skills inspect <name> - show local skill metadata".to_string(),
            "/skills check - check configured hub updates".to_string(),
            "/skills audit [name] - scan local skills".to_string(),
            "/skills reload - refresh skill context".to_string(),
            "/bundles - manage skill bundles".to_string(),
            "/<skill-or-bundle> [args] - submit with a skill or bundle".to_string(),
        ]
        .join("\n")
    }

    pub(crate) fn skills_inspect_text(&self, args: &[&str]) -> String {
        let Some(name) = args.first() else {
            return "usage: /skills inspect <name>".to_string();
        };
        let Some(catalog) = self.current_skill_catalog() else {
            return "No skills found.".to_string();
        };
        match view_skill_value(&catalog, name, None) {
            Ok(value) => {
                let files = value
                    .get("linked_files")
                    .and_then(Value::as_array)
                    .map(Vec::len)
                    .unwrap_or(0);
                let tags = value
                    .get("tags")
                    .and_then(Value::as_array)
                    .map(|values| {
                        values
                            .iter()
                            .filter_map(Value::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "-".to_string());
                [
                    format!("name: {}", json_string(&value, "name")),
                    format!("description: {}", json_string(&value, "description")),
                    format!("source: {}", json_string(&value, "source")),
                    format!("category: {}", json_string(&value, "category")),
                    format!("readiness: {}", json_string(&value, "readiness_status")),
                    format!("platforms: {}", json_string_array(&value, "platforms")),
                    format!("tags: {tags}"),
                    format!("linked_files: {files}"),
                    format!("path: {}", json_string(&value, "path")),
                ]
                .join("\n")
            }
            Err(err) => format!("error: {err:#}"),
        }
    }

    pub(crate) fn skills_check_text(&self) -> String {
        let skill_count = self
            .current_skill_catalog()
            .map(|catalog| catalog.skills.len())
            .unwrap_or(0);
        let bundle_count = self.current_skill_bundles().len();
        format!(
            "no hub update source configured\ninstalled: {skill_count} skills, {bundle_count} bundles"
        )
    }

    pub(crate) fn skills_audit_text(&self, args: &[&str]) -> String {
        let Some(catalog) = self.current_skill_catalog() else {
            return "No skills found.".to_string();
        };
        if let Some(name) = args.first() {
            let normalized = normalize_dynamic_skill_name(name);
            let Some(skill) = catalog.skills.iter().find(|skill| {
                skill.name == *name || normalize_dynamic_skill_name(&skill.name) == normalized
            }) else {
                return format!("unknown skill: {name}");
            };
            return match scan_skill_path(&skill.base_dir) {
                Ok(scan) => format!(
                    "{}: {:?} ({} findings)",
                    skill.name,
                    scan.verdict,
                    scan.findings.len()
                ),
                Err(err) => format!("error: {err:#}"),
            };
        }
        if catalog.skills.is_empty() {
            return "No skills found.".to_string();
        }
        catalog
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
            .join("\n")
    }

    pub(crate) fn skills_reload_text(&self) -> String {
        let skill_count = self
            .current_skill_catalog()
            .map(|catalog| catalog.skills.len())
            .unwrap_or(0);
        let bundle_count = self.current_skill_bundles().len();
        format!("reloaded skills: {skill_count} skills, {bundle_count} bundles")
    }

    pub(crate) fn skills_mutation_text(&self, action: &str, args: &[&str]) -> String {
        if let Err(message) = self.ensure_tui_skill_mutation_allowed(action) {
            return message;
        }
        match action {
            "install" => self.skills_install_text(args),
            "update" => "hub update is not configured for this source".to_string(),
            "uninstall" => self.skills_uninstall_text(args),
            "publish" => "GitHub PR publish requires CLI authentication flow".to_string(),
            "config" => self.skills_config_mutation_text(args),
            _ => "unsupported skill mutation".to_string(),
        }
    }

    pub(crate) fn ensure_tui_skill_mutation_allowed(
        &self,
        action: &str,
    ) -> std::result::Result<(), String> {
        if self.current_mode == RunMode::Plan {
            return Err(format!("/skills {action} is unavailable in plan mode"));
        }
        match self.current_permission_mode {
            PermissionMode::BypassPermissions => Ok(()),
            PermissionMode::DontAsk => Err(format!(
                "permission denied: /skills {action} changes skill state"
            )),
            PermissionMode::Default | PermissionMode::AcceptEdits => Err(format!(
                "/skills {action} changes skill state and requires approval; use /mode bypassPermissions or pevo skill {action}"
            )),
        }
    }

    pub(crate) fn skills_install_text(&self, args: &[&str]) -> String {
        let target = match skill_scope_from_args(args) {
            Ok(target) => target,
            Err(err) => return err,
        };
        let filtered = skill_args_without_scope(args);
        let Some(source) = filtered.first() else {
            return "usage: /skills install <identifier-or-path> [--local|-g|--global] [--name <name>]".to_string();
        };
        let result = install_skill(
            &self.home,
            &self.workdir,
            InstallOptions {
                source: (*source).to_string(),
                target,
                name: skill_option_value(&filtered, "--name").map(ToOwned::to_owned),
                all: args.contains(&"--all"),
                force: args.contains(&"--force"),
            },
        );
        format_skill_mutation_result(result)
    }

    pub(crate) fn skills_uninstall_text(&self, args: &[&str]) -> String {
        let target = match skill_scope_from_args(args) {
            Ok(target) => target,
            Err(err) => return err,
        };
        let filtered = skill_args_without_scope(args);
        let Some(name) = filtered.first() else {
            return "usage: /skills uninstall <name> [--local|-g|--global]".to_string();
        };
        format_skill_mutation_result(remove_installed_skill(
            &self.home,
            &self.workdir,
            target,
            name,
        ))
    }

    pub(crate) fn skills_config_mutation_text(&self, args: &[&str]) -> String {
        let Some(action) = args.first() else {
            return "usage: /skills config enable|disable|set ...".to_string();
        };
        match *action {
            "enable" | "disable" => {
                let target = match skill_scope_from_args(args) {
                    Ok(target) => target,
                    Err(err) => return err,
                };
                let filtered = skill_args_without_scope(args);
                let Some(name) = filtered.get(1) else {
                    return format!("usage: /skills config {action} <name> [--local|-g|--global]");
                };
                format_skill_mutation_result(set_skill_enabled(
                    &self.home,
                    &self.workdir,
                    target,
                    name,
                    *action == "enable",
                ))
            }
            "set" => {
                let target = match skill_scope_from_args(args) {
                    Ok(target) => target,
                    Err(err) => return err,
                };
                let filtered = skill_args_without_scope(args);
                if filtered.len() < 3 {
                    return "usage: /skills config set skills.config.<key> <value> [--local|-g|--global]".to_string();
                }
                let value = serde_json::from_str::<Value>(filtered[2])
                    .unwrap_or_else(|_| Value::String(filtered[2].to_string()));
                format_skill_mutation_result(set_skill_config_value(
                    &self.home,
                    &self.workdir,
                    target,
                    filtered[1],
                    value,
                ))
            }
            other => format!("unknown /skills config action: {other}"),
        }
    }

    pub(crate) fn bundles_command_text(&self, args: Option<&str>) -> String {
        match args.map(str::trim).filter(|value| !value.is_empty()) {
            None => [
                "Skill bundles",
                "/bundles list - list installed bundles",
                "/<bundle> [args] - submit with a bundle",
            ]
            .join("\n"),
            Some("list") => self.bundles_status_text(),
            Some(_) => "Supported bundle commands: /bundles, /bundles list".to_string(),
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

    pub(crate) fn bundles_status_text(&self) -> String {
        let bundles = self.current_skill_bundles();
        if bundles.is_empty() {
            return "No skill bundles found.".to_string();
        }
        bundles
            .iter()
            .map(|bundle| {
                format!(
                    "{}: {} [{}]",
                    bundle.slug,
                    bundle.description,
                    bundle.skills.join(", ")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub(crate) fn skill_or_bundle_marker(&self, name: &str, args: &str) -> Option<String> {
        let normalized = normalize_dynamic_skill_name(name);
        for bundle in self.current_skill_bundles() {
            if bundle.slug == normalized || normalize_dynamic_skill_name(&bundle.name) == normalized
            {
                return Some(skill_prompt_marker(&bundle.slug, args));
            }
        }
        let catalog = self.current_skill_catalog()?;
        catalog
            .skills
            .iter()
            .any(|skill| {
                skill.name == name || normalize_dynamic_skill_name(&skill.name) == normalized
            })
            .then(|| skill_prompt_marker(name, args))
    }

    pub(crate) fn permissions_status_text(&self) -> Result<String> {
        let options = self.run_options(String::new());
        let value = permission_rules_value(&options, ConfigScope::Local)?;
        let permissions = &value["permissions"];
        let mut lines = vec![
            format!("mode: {}", self.current_mode.as_str()),
            format!("permission_mode: {}", self.current_permission_mode.as_str()),
            format!(
                "approval_policy: {}",
                permissions["approval_policy"]
                    .as_str()
                    .unwrap_or("on-request")
            ),
            format!(
                "approvals_reviewer: {}",
                permissions["approvals_reviewer"].as_str().unwrap_or("user")
            ),
            format!(
                "default_permissions: {}",
                permissions["default_permissions"]
                    .as_str()
                    .unwrap_or(":workspace")
            ),
            format!(
                "path: {}",
                value["path"].as_str().unwrap_or(".psychevo/config.toml")
            ),
        ];
        lines.push("profiles:".to_string());
        let profiles = permissions["profiles"]
            .as_object()
            .cloned()
            .unwrap_or_default();
        if profiles.is_empty() {
            lines.push("  (none)".to_string());
        } else {
            for name in profiles.keys() {
                lines.push(format!("  {name}"));
            }
        }
        lines.push("exec_policy:".to_string());
        let rules = permissions["exec_policy"]["rules"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if rules.is_empty() {
            lines.push("  (none)".to_string());
        } else {
            for rule in rules {
                let prefix = rule["prefix"]
                    .as_array()
                    .map(|values| format_exec_prefix_for_status(values))
                    .unwrap_or_else(|| "-".to_string());
                lines.push(format!(
                    "  {} -> {}",
                    prefix,
                    rule["decision"].as_str().unwrap_or("-")
                ));
            }
        }
        Ok(lines.join("\n"))
    }

    pub(crate) fn sandbox_status_text(&self) -> Result<String> {
        let options = self.run_options(String::new());
        Ok(psychevo_runtime::sandbox_status_text(
            &options,
            self.current_mode,
        )?)
    }

    pub(crate) fn agents_status_text(&self) -> String {
        let Some(catalog) = self.current_agent_catalog() else {
            return "Agents disabled.".to_string();
        };
        let mut sections = Vec::new();
        if catalog.agents.is_empty() {
            sections.push("Library\nNo agents found.".to_string());
        } else {
            sections.push(format!(
                "Library\n{}",
                catalog
                    .agents
                    .iter()
                    .map(|agent| format!("{}: {}", agent.name, agent.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if let Some(parent) = self.current_session.as_deref() {
            let store = self.state_runtime.store();
            let value = agent_status_value(Some(store), Some(parent), false);
            let running = value
                .get("agents")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default();
            if running.is_empty() {
                sections.push("Running/Completed\nNo child agents for this session.".to_string());
            } else {
                sections.push(format!(
                    "Running/Completed\n{}",
                    running
                        .iter()
                        .map(|agent| format!(
                            "{}\t{}\t{}",
                            agent.get("id").and_then(Value::as_str).unwrap_or_default(),
                            agent
                                .get("agent_name")
                                .and_then(Value::as_str)
                                .unwrap_or_default(),
                            agent
                                .get("status")
                                .and_then(Value::as_str)
                                .unwrap_or_default()
                        ))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            }
        }
        sections.join("\n\n")
    }

    pub(crate) fn help_status_text(&self) -> String {
        format_slash_help_with_config(self.current_skill_count(), &self.slash_config)
    }

    pub(crate) fn help_panel(&self) -> HelpPanel {
        HelpPanel::new(slash_help_sections_with_config(
            self.current_skill_count(),
            &self.slash_config,
        ))
    }

    pub(crate) fn current_skill_count(&self) -> Option<usize> {
        self.current_skill_catalog()
            .map(|catalog| catalog.skills.len())
    }

    pub(crate) fn stats_status_text(&self) -> Result<String> {
        let report = usage_stats(StatsOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            all: false,
            days: None,
            limit: 5,
        })?;
        let totals = report.get("totals").unwrap_or(&Value::Null);
        Ok(format!(
            "sessions: {}  messages: {}  tokens: {}  cost: {}",
            json_i64(totals, "sessions"),
            json_i64(totals, "messages"),
            json_i64(totals, "reported_total_tokens"),
            format_nanodollars(json_i64(totals, "estimated_cost_nanodollars"))
        ))
    }

    pub(crate) fn write_tui_export(
        &self,
        options: &crate::tui::slash::TuiExportOptions,
    ) -> Result<SessionExportWriteResult> {
        let session_id = self
            .current_session
            .as_deref()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        let output = self.resolve_tui_export_path(
            options.path.as_deref(),
            options.format,
            SessionArtifactKind::Export,
            session_id,
        );
        let store = self.state_runtime.store();
        Ok(write_session_export(
            store,
            session_id,
            &output,
            SessionExportOptions {
                format: options.format,
                include: options.include.clone(),
                artifact_kind: SessionArtifactKind::Export,
            },
        )?)
    }

    pub(crate) fn write_tui_share(
        &self,
        options: &crate::tui::slash::TuiShareOptions,
    ) -> Result<SessionExportWriteResult> {
        let session_id = self
            .current_session
            .as_deref()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        let output = self.resolve_tui_export_path(
            options.path.as_deref(),
            SessionExportFormat::Markdown,
            SessionArtifactKind::Share,
            session_id,
        );
        let store = self.state_runtime.store();
        Ok(write_session_export(
            store,
            session_id,
            &output,
            SessionExportOptions {
                format: SessionExportFormat::Markdown,
                include: options.include.clone(),
                artifact_kind: SessionArtifactKind::Share,
            },
        )?)
    }

    pub(crate) fn resolve_tui_export_path(
        &self,
        path: Option<&str>,
        format: SessionExportFormat,
        artifact_kind: SessionArtifactKind,
        session_id: &str,
    ) -> PathBuf {
        let path = path.map(PathBuf::from).unwrap_or_else(|| {
            self.workdir.join(default_session_export_filename(
                session_id,
                format,
                artifact_kind,
            ))
        });
        if path.is_absolute() {
            path
        } else {
            self.workdir.join(path)
        }
    }

    pub(crate) fn context_status_snapshot(
        &self,
        live: Option<&ContextSnapshot>,
    ) -> Result<ContextSnapshot> {
        if let Some(snapshot) = live {
            return Ok(snapshot.clone());
        }
        if let Some(snapshot) = self.last_context_snapshot.as_ref() {
            return Ok(snapshot.clone());
        }
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        Ok(context_snapshot(ContextOptions {
            state: self.state_runtime.clone(),
            workdir: self.workdir.clone(),
            session,
            config_path: self.config_path.clone(),
            inherited_env: Some(self.env_map.clone()),
        })?)
    }

    pub(crate) fn reload_context_for_current_session(
        &self,
        ui: &FullscreenUi<'_>,
    ) -> Result<psychevo_runtime::ReloadContextResult> {
        if ui.running.is_some() {
            return Err(anyhow!("finish the current turn before reloading context"));
        }
        let session = self
            .current_session
            .clone()
            .ok_or_else(|| anyhow!("no session context yet"))?;
        Ok(reload_session_context(ReloadContextOptions {
            state: self.state_runtime.clone(),
            session,
            config_path: self.config_path.clone(),
            mode: Some(self.current_mode),
            inherited_env: Some(self.env_map.clone()),
            agent: self.current_agent.clone(),
            no_agents: self.no_agents,
            no_skills: self.no_skills,
            invalidation_reason: "manual_reload".to_string(),
            notice: None,
        })?)
    }

    pub(crate) async fn submit_prompt(&mut self, prompt: String) -> Result<()> {
        let stdout = Arc::new(Mutex::new(io::stdout()));
        let turn = Arc::new(Mutex::new(TurnPrinter::new(
            self.renderer,
            self.thinking_visible,
            self.debug,
        )));
        {
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            writeln!(stdout, "Prompt: {prompt}")?;
        }
        let turn_for_sink = Arc::clone(&turn);
        let stdout_for_sink = Arc::clone(&stdout);
        let sink: GatewayEventSink = Arc::new(move |event| {
            let mut turn = turn_for_sink.lock().expect("turn lock poisoned");
            let mut stdout = stdout_for_sink.lock().expect("stdout lock poisoned");
            let _ = turn.render_gateway_event(&event, &mut *stdout);
        });
        let options = self.run_options(prompt);
        let source = self.gateway_source();
        let bind_source = self.canonical_gateway_source();
        let reset_source_binding = self.force_new_once && self.current_session.is_none();
        let result = self
            .gateway
            .send_turn(SendTurnRequest {
                thread_id: options.session.clone(),
                source: Some(source),
                bind_source: Some(bind_source),
                reset_source_binding,
                input: Vec::new(),
                options,
                runtime_source: Some("tui".to_string()),
                continue_sources: TUI_CONTINUE_SESSION_SOURCES
                    .iter()
                    .map(|source| (*source).to_string())
                    .collect(),
                stream: None,
                event_sink: Some(sink),
                control_handle: None,
                control: None,
                lineage: None,
            })
            .await?
            .result;
        self.last_context_snapshot = result.context_snapshot.clone();
        {
            let mut turn = turn.lock().expect("turn lock poisoned");
            let mut stdout = stdout.lock().expect("stdout lock poisoned");
            turn.finish(&mut *stdout)?;
        }
        self.current_session = Some(result.session_id);
        self.reset_live_agent_reload_poll();
        self.refresh_current_session_title()?;
        self.clear_new_session_draft();
        let success = result.outcome == Outcome::Normal && result.tool_failures == 0;
        if !success {
            self.had_error = true;
        }
        Ok(())
    }
}

pub(crate) fn format_exec_prefix_for_status(values: &[Value]) -> String {
    values
        .iter()
        .filter_map(|value| match value {
            Value::String(raw) => Some(raw.clone()),
            Value::Array(alternatives) => Some(format!(
                "[{}]",
                alternatives
                    .iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("|")
            )),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(" ")
}
