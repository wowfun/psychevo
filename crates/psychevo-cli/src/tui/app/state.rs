#[allow(unused_imports)]
pub(crate) use super::*;
static NEXT_TUI_DRAFT_SOURCE: AtomicU64 = AtomicU64::new(1);

pub(crate) struct TuiApp {
    pub(crate) env_map: BTreeMap<String, String>,
    pub(crate) home: PathBuf,
    pub(crate) state_path: PathBuf,
    pub(crate) state: TuiState,
    pub(crate) model_state_path: PathBuf,
    pub(crate) model_state: ModelState,
    pub(crate) state_runtime: StateRuntime,
    pub(crate) gateway: Gateway,
    pub(crate) db_path: PathBuf,
    pub(crate) config_path: Option<PathBuf>,
    pub(crate) cwd: PathBuf,
    pub(crate) cwd_key: String,
    pub(crate) current_session: Option<String>,
    pub(crate) current_session_title: Option<String>,
    pub(crate) force_new_once: bool,
    pub(crate) draft_source_raw_id: Option<String>,
    pub(crate) current_model: Option<String>,
    pub(crate) current_variant: Option<String>,
    pub(crate) selected_model: Option<ConfiguredModel>,
    pub(crate) current_mode: RunMode,
    pub(crate) current_permission_mode: PermissionMode,
    pub(crate) startup_agent: Option<String>,
    pub(crate) current_agent: Option<String>,
    pub(crate) current_agent_explicit_default: bool,
    pub(crate) no_agents: bool,
    pub(crate) no_skills: bool,
    pub(crate) skill_inputs: Vec<String>,
    pub(crate) thinking_visible: bool,
    pub(crate) raw_visible: bool,
    pub(crate) clipboard: ClipboardSink,
    pub(crate) renderer: TuiRenderer,
    pub(crate) debug: bool,
    pub(crate) had_error: bool,
    pub(crate) last_context_snapshot: Option<ContextSnapshot>,
    pub(crate) model_catalog: ModelCatalogCache,
    pub(crate) clipboard_result_tx: std::sync::mpsc::Sender<Result<(), String>>,
    pub(crate) clipboard_result_rx: std::sync::mpsc::Receiver<Result<(), String>>,
    pub(crate) clipboard_copies_in_flight: usize,
    pub(crate) slash_config: EffectiveSlashConfig,
    pub(crate) side_conversation: Option<SideConversationState>,
    pub(crate) last_live_agent_reload_check: Option<Instant>,
    pub(crate) last_gateway_live_event_seq: i64,
    pub(crate) gateway_live_snapshot_revisions: BTreeMap<String, i64>,
    pub(crate) session_browser_limits: BTreeMap<String, usize>,
    pub(crate) side_cleanup_task: Option<SideCleanupTask>,
    pub(crate) compaction_task: Option<CompactionTask>,
    pub(crate) diff_task: Option<DiffTask>,
    pub(crate) journey_profile: TuiJourneyProfileProbe,
}

pub(crate) struct SideConversationState {
    pub(crate) parent_session: String,
    pub(crate) parent_session_title: Option<String>,
    pub(crate) parent_model: Option<String>,
    pub(crate) parent_variant: Option<String>,
    pub(crate) parent_mode: RunMode,
    pub(crate) parent_permission_mode: PermissionMode,
    pub(crate) parent_agent: Option<String>,
    pub(crate) parent_agent_explicit_default: bool,
    pub(crate) side_thread_id: String,
}

pub(crate) struct SideCleanupTask {
    pub(crate) task: JoinHandle<std::result::Result<usize, String>>,
}

pub(crate) struct CompactionTask {
    pub(crate) session_id: String,
    pub(crate) command_echo: Option<String>,
    pub(crate) manual: bool,
    pub(crate) task: JoinHandle<std::result::Result<CompactionResult, String>>,
}

pub(crate) struct DiffTask {
    pub(crate) task: JoinHandle<std::result::Result<WorkspaceDiff, String>>,
}

impl TuiApp {
    pub(crate) fn begin_new_session_draft(&mut self) {
        self.current_session = None;
        self.reset_live_agent_reload_poll();
        self.current_session_title = None;
        self.force_new_once = true;
        self.draft_source_raw_id = Some(new_tui_draft_source_raw_id(&self.cwd_key));
    }

    pub(crate) fn clear_new_session_draft(&mut self) {
        self.force_new_once = false;
        self.draft_source_raw_id = None;
    }

    pub(crate) fn canonical_gateway_source(&self) -> GatewaySource {
        self.gateway_source_for_raw_id(self.cwd_key.clone(), "TUI")
    }

    pub(crate) fn gateway_source(&self) -> GatewaySource {
        if self.force_new_once
            && self.current_session.is_none()
            && let Some(raw_id) = self.draft_source_raw_id.clone()
        {
            return self.gateway_source_for_raw_id(raw_id, "TUI draft");
        }
        self.canonical_gateway_source()
    }

    fn gateway_source_for_raw_id(&self, raw_id: String, visible_name: &str) -> GatewaySource {
        GatewaySource::new("tui", raw_id)
            .process()
            .with_visible_name(visible_name)
            .with_raw_identity(serde_json::json!({
                "kind": "tui",
                "cwd": self.cwd.display().to_string(),
                "canonicalRawId": self.cwd_key.clone(),
            }))
    }

    pub(crate) fn current_skill_catalog(&self) -> Option<SkillCatalog> {
        discover_skills(&SkillDiscoveryOptions {
            home: self.home.clone(),
            cwd: self.cwd.clone(),
            config_path: self.config_path.clone(),
            env: self.env_map.clone(),
            explicit_inputs: self.skill_inputs.clone(),
            additional_roots: Vec::new(),
            no_skills: self.no_skills,
        })
        .ok()
    }

    pub(crate) fn current_agent_catalog(&self) -> Option<AgentCatalog> {
        if self.no_agents {
            return None;
        }
        discover_agents(&AgentDiscoveryOptions {
            home: self.home.clone(),
            cwd: self.cwd.clone(),
            env: self.env_map.clone(),
            explicit_inputs: self.current_agent.iter().cloned().collect(),
            no_agents: self.no_agents,
        })
        .ok()
    }

    pub(crate) fn current_skill_bundles(&self) -> Vec<SkillBundle> {
        if self.no_skills {
            return Vec::new();
        }
        list_skill_bundles(&self.home, &self.cwd).unwrap_or_default()
    }

    pub(crate) fn slash_items(&self) -> Vec<SlashMenuItem> {
        let mut items = configured_slash_menu_items(&self.slash_config);
        let mut dynamic_names = BTreeSet::new();
        for bundle in self.current_skill_bundles() {
            let command = format!("/{}", bundle.slug);
            dynamic_names.insert(bundle.slug.clone());
            items.push(SlashMenuItem {
                command: command.clone(),
                description: bundle.description,
                upcoming: false,
                aliases: Vec::new(),
                replacement: command.clone(),
                completion: command,
                configured_alias: false,
            });
        }
        if let Some(catalog) = self.current_skill_catalog() {
            for skill in catalog.skills {
                if !skill.enabled
                    || skill.disable_model_invocation
                    || !skill.supported_on_current_platform
                    || !skill.collision_group.is_empty()
                {
                    continue;
                }
                if dynamic_names.contains(&skill.name) {
                    continue;
                }
                let command = format!("/{}", skill.name);
                items.push(SlashMenuItem {
                    command: command.clone(),
                    description: skill.description,
                    upcoming: false,
                    aliases: Vec::new(),
                    replacement: command.clone(),
                    completion: command,
                    configured_alias: false,
                });
            }
        }
        items
    }

    pub(crate) fn slash_menu_items(&self, input: &str) -> Vec<SlashMenuItem> {
        slash_menu_items_from(input, &self.slash_items())
    }

    pub(crate) fn skill_search_matches(&self, query: &str) -> Vec<SkillSearchMatch> {
        let Some(catalog) = self.current_skill_catalog() else {
            return Vec::new();
        };
        let query = query.trim().to_lowercase();
        let mut matches = catalog
            .skills
            .into_iter()
            .filter_map(|skill| {
                skill_match_rank(&skill.name, &skill.description, &query).map(|rank| {
                    (
                        rank,
                        SkillSearchMatch {
                            name: skill.name,
                            description: skill.description,
                            source_label: psychevo_runtime::skill_source_display_label(Some(
                                skill.source.as_str(),
                            ))
                            .map(ToString::to_string),
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_rank, left), (right_rank, right)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        matches
            .into_iter()
            .take(FILE_POPUP_MAX_ROWS)
            .map(|(_, entry)| entry)
            .collect()
    }

    pub(crate) fn agent_search_matches(&self, query: &str) -> Vec<AgentSearchMatch> {
        let query = query.trim().to_lowercase();
        let Some(catalog) = self.current_agent_catalog() else {
            return Vec::new();
        };
        let mut matches = catalog
            .agents
            .into_iter()
            .filter_map(|agent| {
                skill_match_rank(&agent.name, &agent.description, &query).map(|rank| {
                    (
                        rank,
                        AgentSearchMatch {
                            name: agent.name,
                            description: agent.description,
                            source_label: psychevo_runtime::agent_source_display_label(Some(
                                agent.source.as_str(),
                            ))
                            .map(ToString::to_string),
                        },
                    )
                })
            })
            .collect::<Vec<_>>();
        matches.sort_by(|(left_rank, left), (right_rank, right)| {
            left_rank
                .cmp(right_rank)
                .then_with(|| left.name.cmp(&right.name))
        });
        matches
            .into_iter()
            .take(FILE_POPUP_MAX_ROWS)
            .map(|(_, entry)| entry)
            .collect()
    }

    pub(crate) fn sync_agent_popup(&self, ui: &mut FullscreenUi<'_>) {
        if ui.shell_mode || ui.current_skill_token().is_some() {
            ui.close_agent_popup();
            return;
        }
        let Some(token) = ui.current_agent_token() else {
            ui.sync_agent_popup(Vec::new());
            return;
        };
        ui.sync_agent_popup(self.agent_search_matches(&token.query));
    }

    pub(crate) fn sync_skill_popup(&self, ui: &mut FullscreenUi<'_>) {
        if ui.shell_mode || ui.current_file_token().is_some() || ui.current_agent_token().is_some()
        {
            ui.close_skill_popup();
            return;
        }
        let Some(token) = ui.current_skill_token() else {
            ui.sync_skill_popup(Vec::new());
            return;
        };
        ui.sync_skill_popup(self.skill_search_matches(&token.query));
    }
}

fn new_tui_draft_source_raw_id(cwd_key: &str) -> String {
    let sequence = NEXT_TUI_DRAFT_SOURCE.fetch_add(1, Ordering::Relaxed);
    format!("{cwd_key}:draft:{sequence}")
}

pub(crate) fn skill_match_rank(name: &str, description: &str, query: &str) -> Option<(u8, usize)> {
    if query.is_empty() {
        return Some((3, 0));
    }
    let name_lower = name.to_lowercase();
    let description_lower = description.to_lowercase();
    if name_lower.starts_with(query) {
        return Some((0, name_lower.len().saturating_sub(query.len())));
    }
    if let Some(index) = name_lower.find(query) {
        return Some((1, index));
    }
    if let Some(score) = fuzzy_subsequence_score(&name_lower, query) {
        return Some((2, score));
    }
    description_lower.find(query).map(|index| (4, index))
}
